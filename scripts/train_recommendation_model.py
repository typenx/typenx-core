"""Train the production Typenx recommender artifact with the local GPU.

The trainer reads Typenx library rows from SQLite, trains an implicit-feedback
matrix factorization model with PyTorch DirectML, and writes precomputed
per-user recommendations consumed by `TYPENX_RECOMMENDER_MODEL_PATH`.
"""

from __future__ import annotations

import argparse
import json
import math
import sqlite3
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import torch
from torch import nn


STATUS_WEIGHT = {
    "completed": 1.0,
    "watching": 0.8,
    "planning": 0.2,
    "paused": -0.25,
    "dropped": -1.25,
}


class MatrixFactorization(nn.Module):
    def __init__(self, users: int, items: int, factors: int) -> None:
        super().__init__()
        self.user_factors = nn.Embedding(users, factors)
        self.item_factors = nn.Embedding(items, factors)
        self.user_bias = nn.Embedding(users, 1)
        self.item_bias = nn.Embedding(items, 1)

    def forward(self, users: torch.Tensor, items: torch.Tensor) -> torch.Tensor:
        user_vec = self.user_factors(users)
        item_vec = self.item_factors(items)
        return (user_vec * item_vec).sum(dim=1) + self.user_bias(users).squeeze(1) + self.item_bias(items).squeeze(1)


def directml_device() -> tuple[torch.device, str]:
    import torch_directml

    device = torch_directml.device()
    return device, "directml"


def load_rows(database: Path) -> tuple[list[sqlite3.Row], list[dict]]:
    connection = sqlite3.connect(database)
    connection.row_factory = sqlite3.Row
    try:
        rows = list(
            connection.execute(
                """
                SELECT user_id, provider_anime_id, title, status, score,
                       progress_episodes, total_episodes
                FROM anime_list_entries
                """
            )
        )
        candidates = []
        for cache_row in connection.execute("SELECT payload_json FROM metadata_cache"):
            try:
                payload = json.loads(cache_row["payload_json"])
            except json.JSONDecodeError:
                continue
            if isinstance(payload.get("items"), list):
                candidates.extend(payload["items"])
            elif isinstance(payload.get("id"), str) and isinstance(payload.get("title"), str):
                candidates.append(payload)
        return rows, candidates
    finally:
        connection.close()


def label_for(row: sqlite3.Row) -> float:
    score = row["score"]
    score_weight = 0.0 if score is None else (float(score) - 5.0) / 5.0
    status_weight = STATUS_WEIGHT.get(str(row["status"]).lower(), 0.0)
    total = row["total_episodes"] or 0
    progress = 0.0 if total <= 0 else min(float(row["progress_episodes"] or 0) / float(total), 1.0)
    return 1.0 if score_weight + status_weight + progress * 0.35 > 0.35 else 0.0


def build_dataset(rows: list[sqlite3.Row], candidates: list[dict], negatives_per_positive: int, seed: int):
    user_ids = sorted({row["user_id"] for row in rows})
    item_ids = sorted(
        {row["provider_anime_id"] for row in rows}
        | {str(candidate["id"]) for candidate in candidates if candidate.get("id")}
    )
    user_index = {value: index for index, value in enumerate(user_ids)}
    item_index = {value: index for index, value in enumerate(item_ids)}
    titles = {}
    seen = defaultdict(set)
    samples = []

    for row in rows:
        user = user_index[row["user_id"]]
        item = item_index[row["provider_anime_id"]]
        titles[row["provider_anime_id"]] = row["title"]
        seen[user].add(item)
        samples.append((user, item, label_for(row)))

    candidate_previews = {}
    for candidate in candidates:
        item_id = str(candidate.get("id") or "")
        title = str(candidate.get("title") or item_id)
        if not item_id or item_id in candidate_previews:
            continue
        titles[item_id] = title
        candidate_previews[item_id] = {
            "id": item_id,
            "title": title,
            "poster": candidate.get("poster"),
            "banner": candidate.get("banner"),
            "synopsis": candidate.get("synopsis"),
            "score": candidate.get("score"),
            "year": candidate.get("year"),
            "content_type": candidate.get("content_type") or "anime",
            "genres": candidate.get("genres") or [],
            "season_entries": candidate.get("season_entries") or [],
        }

    rng = np.random.default_rng(seed)
    all_items = np.arange(len(item_ids), dtype=np.int64)
    positives = [sample for sample in samples if sample[2] >= 0.5]
    for user, _item, _label in positives:
        available = np.setdiff1d(all_items, np.fromiter(seen[user], dtype=np.int64), assume_unique=False)
        if available.size == 0:
            continue
        negative_count = min(negatives_per_positive, available.size)
        for item in rng.choice(available, size=negative_count, replace=False):
            samples.append((user, int(item), 0.0))

    rng.shuffle(samples)
    users = np.array([sample[0] for sample in samples], dtype=np.int64)
    items = np.array([sample[1] for sample in samples], dtype=np.int64)
    labels = np.array([sample[2] for sample in samples], dtype=np.float32)
    return user_ids, item_ids, titles, candidate_previews, seen, users, items, labels


def train(args: argparse.Namespace) -> dict:
    rows, candidates = load_rows(args.database)
    if not rows:
        raise SystemExit(f"No anime_list_entries found in {args.database}")

    user_ids, item_ids, titles, candidate_previews, seen, users, items, labels = build_dataset(
        rows, candidates, args.negatives_per_positive, args.seed
    )
    device, backend = directml_device()
    torch.manual_seed(args.seed)
    model = MatrixFactorization(len(user_ids), len(item_ids), args.factors).to(device)
    optimizer = torch.optim.SGD(model.parameters(), lr=args.lr, momentum=0.9)
    loss_fn = nn.MSELoss()
    started = time.perf_counter()

    for epoch in range(args.epochs):
        order = np.random.permutation(len(labels))
        losses = []
        for offset in range(0, len(labels), args.batch_size):
            batch = order[offset : offset + args.batch_size]
            user_batch = torch.as_tensor(users[batch], dtype=torch.long, device=device)
            item_batch = torch.as_tensor(items[batch], dtype=torch.long, device=device)
            label_batch = torch.as_tensor(labels[batch], dtype=torch.float32, device=device)
            optimizer.zero_grad(set_to_none=True)
            prediction = torch.sigmoid(model(user_batch, item_batch))
            loss = loss_fn(prediction, label_batch)
            loss.backward()
            optimizer.step()
            losses.append(float(loss.detach().cpu()))
        print(f"epoch={epoch + 1} loss={sum(losses) / len(losses):.4f}")

    artifact_users = {}
    with torch.no_grad():
        item_tensor = torch.arange(len(item_ids), dtype=torch.long, device=device)
        for user_index, user_id in enumerate(user_ids):
            user_tensor = torch.full((len(item_ids),), user_index, dtype=torch.long, device=device)
            scores = torch.sigmoid(model(user_tensor, item_tensor)).detach().cpu().numpy()
            ranked = np.argsort(-scores)
            recommendations = []
            for item_index_value in ranked:
                if int(item_index_value) in seen[user_index]:
                    continue
                item_id = item_ids[int(item_index_value)]
                preview = candidate_previews.get(
                    item_id,
                    {
                        "id": item_id,
                        "title": titles.get(item_id, item_id),
                        "poster": None,
                        "banner": None,
                        "synopsis": None,
                        "score": None,
                        "year": None,
                        "content_type": "anime",
                        "genres": [],
                        "season_entries": [],
                    },
                )
                recommendations.append({
                    **preview,
                    "recommendation_score": float(round(float(scores[item_index_value]), 6)),
                    "reasons": ["gpu trained implicit feedback"],
                })
                if len(recommendations) >= args.recommendations_per_user:
                    break
            artifact_users[user_id] = recommendations

    return {
        "version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "backend": backend,
        "database": str(args.database),
        "users_trained": len(user_ids),
        "items_trained": len(item_ids),
        "metadata_candidates": len(candidate_previews),
        "samples": len(labels),
        "elapsed_seconds": round(time.perf_counter() - started, 3),
        "users": artifact_users,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", type=Path, default=Path("typenx.sqlite"))
    parser.add_argument("--output", type=Path, default=Path("recommendations.model.json"))
    parser.add_argument("--factors", type=int, default=64)
    parser.add_argument("--epochs", type=int, default=25)
    parser.add_argument("--batch-size", type=int, default=4096)
    parser.add_argument("--lr", type=float, default=0.05)
    parser.add_argument("--negatives-per-positive", type=int, default=4)
    parser.add_argument("--recommendations-per-user", type=int, default=100)
    parser.add_argument("--seed", type=int, default=7)
    return parser.parse_args()


if __name__ == "__main__":
    parsed = parse_args()
    artifact = train(parsed)
    parsed.output.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
    print(
        json.dumps(
            {
                "output": str(parsed.output),
                "backend": artifact["backend"],
                "users_trained": artifact["users_trained"],
                "items_trained": artifact["items_trained"],
                "samples": artifact["samples"],
                "elapsed_seconds": artifact["elapsed_seconds"],
            },
            indent=2,
        )
    )
