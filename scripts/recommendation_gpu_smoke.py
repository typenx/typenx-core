"""GPU smoke test for the Typenx recommendation model path.

This trains a compact implicit-feedback matrix factorization model on synthetic
Typenx-style user/anime interactions. It is intentionally small enough to run as
a local health check, but it exercises the same tensor operations used by the
future offline recommender trainer.
"""

from __future__ import annotations

import argparse
import json
import platform
import random
import time
from dataclasses import dataclass

import numpy as np
import torch
from torch import nn


@dataclass(frozen=True)
class DeviceInfo:
    name: str
    torch_device: torch.device
    backend: str


def select_device(prefer_gpu: bool) -> DeviceInfo:
    if prefer_gpu:
        try:
            import torch_directml

            device = torch_directml.device()
            return DeviceInfo(str(device), device, "directml")
        except Exception as error:  # pragma: no cover - diagnostic path
            print(f"DirectML unavailable, falling back to CPU: {error}")

    return DeviceInfo("cpu", torch.device("cpu"), "cpu")


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
        dot = (user_vec * item_vec).sum(dim=1)
        return dot + self.user_bias(users).squeeze(1) + self.item_bias(items).squeeze(1)


def make_dataset(users: int, items: int, interactions: int, seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(seed)
    user_taste = rng.normal(size=(users, 12)).astype(np.float32)
    item_traits = rng.normal(size=(items, 12)).astype(np.float32)
    user_ids = rng.integers(0, users, size=interactions, dtype=np.int64)
    item_ids = rng.integers(0, items, size=interactions, dtype=np.int64)
    affinity = (user_taste[user_ids] * item_traits[item_ids]).sum(axis=1)
    noise = rng.normal(scale=1.5, size=interactions)
    ratings = np.clip(5.0 + affinity + noise, 0.0, 10.0).astype(np.float32)
    labels = (ratings >= 7.0).astype(np.float32)
    return user_ids, item_ids, labels


def train(args: argparse.Namespace) -> dict[str, object]:
    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    device = select_device(args.gpu)
    user_ids, item_ids, labels = make_dataset(args.users, args.items, args.interactions, args.seed)

    model = MatrixFactorization(args.users, args.items, args.factors).to(device.torch_device)
    optimizer = torch.optim.SGD(model.parameters(), lr=args.lr, momentum=0.9)
    loss_fn = nn.MSELoss()
    start = time.perf_counter()

    for epoch in range(args.epochs):
        order = np.random.permutation(args.interactions)
        losses: list[float] = []
        for offset in range(0, args.interactions, args.batch_size):
            batch = order[offset : offset + args.batch_size]
            users = torch.as_tensor(user_ids[batch], dtype=torch.long, device=device.torch_device)
            items = torch.as_tensor(item_ids[batch], dtype=torch.long, device=device.torch_device)
            target = torch.as_tensor(labels[batch], dtype=torch.float32, device=device.torch_device)

            optimizer.zero_grad(set_to_none=True)
            predictions = torch.sigmoid(model(users, items))
            loss = loss_fn(predictions, target)
            loss.backward()
            optimizer.step()
            losses.append(float(loss.detach().cpu()))

        print(f"epoch={epoch + 1} loss={sum(losses) / len(losses):.4f}")

    elapsed = time.perf_counter() - start
    with torch.no_grad():
        sample_users = torch.arange(0, min(4, args.users), dtype=torch.long, device=device.torch_device)
        sample_items = torch.arange(0, min(16, args.items), dtype=torch.long, device=device.torch_device)
        scores = model(
            sample_users.repeat_interleave(sample_items.numel()),
            sample_items.repeat(sample_users.numel()),
        ).reshape(sample_users.numel(), sample_items.numel())
        top_items = torch.topk(scores, k=min(5, sample_items.numel()), dim=1).indices.cpu().tolist()

    return {
        "backend": device.backend,
        "device": device.name,
        "platform": platform.platform(),
        "torch": torch.__version__,
        "users": args.users,
        "items": args.items,
        "interactions": args.interactions,
        "epochs": args.epochs,
        "elapsed_seconds": round(elapsed, 3),
        "interactions_per_second": round(args.interactions * args.epochs / elapsed, 1),
        "sample_top_items": top_items,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpu", dest="gpu", action="store_false", help="Force CPU instead of DirectML")
    parser.add_argument("--users", type=int, default=4096)
    parser.add_argument("--items", type=int, default=8192)
    parser.add_argument("--interactions", type=int, default=131072)
    parser.add_argument("--factors", type=int, default=64)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=4096)
    parser.add_argument("--lr", type=float, default=0.01)
    parser.add_argument("--seed", type=int, default=7)
    parser.set_defaults(gpu=True)
    return parser.parse_args()


if __name__ == "__main__":
    result = train(parse_args())
    print(json.dumps(result, indent=2))
