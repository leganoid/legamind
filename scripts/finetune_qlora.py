"""LoRA-дообучение локальной модели на датасете Legamind.

Датасет — JSONL, каждая строка:
    {"messages": [{"role": "user", "content": "..."}, {"role": "assistant", "content": "..."}]}

Пример:
    python scripts/finetune_qlora.py --dataset %APPDATA%/com.legamind.app/datasets/foo.jsonl \
        --base Qwen/Qwen2.5-1.5B-Instruct --output adapters/qwen-1.5b-lora --epochs 2

Требует: torch, transformers, peft, datasets. Для Intel GPU — torch с XPU
(`pip install torch --index-url https://download.pytorch.org/whl/xpu`) и переменная
ONEAPI_DEVICE_SELECTOR=level_zero:0 при необходимости.

После обучения адаптер сливается и конвертируется в OpenVINO IR:
    python scripts/export_openvino.py --base Qwen/Qwen2.5-1.5B-Instruct \
        --adapter adapters/qwen-1.5b-lora --output C:/Models/qwen-1.5b-lora-int4-ov
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="LoRA fine-tuning для Legamind")
    parser.add_argument("--dataset", required=True, help="JSONL с messages")
    parser.add_argument("--base", required=True, help="HF id или путь к исходным весам")
    parser.add_argument("--output", required=True, help="куда сохранить LoRA-адаптер")
    parser.add_argument("--epochs", type=float, default=2.0)
    parser.add_argument("--lr", type=float, default=1e-4)
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--alpha", type=int, default=32)
    parser.add_argument("--dropout", type=float, default=0.05)
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--grad-accum", type=int, default=8)
    parser.add_argument("--max-len", type=int, default=1024)
    parser.add_argument("--save-steps", type=int, default=50)
    parser.add_argument("--load-4bit", action="store_true", help="bitsandbytes 4-bit (только CUDA)")
    parser.add_argument("--device", default="auto", choices=["auto", "cpu", "xpu", "cuda"])
    return parser.parse_args()


def pick_device(requested: str) -> str:
    import torch

    if requested != "auto":
        return requested
    if getattr(torch, "xpu", None) is not None and torch.xpu.is_available():
        return "xpu"
    if torch.cuda.is_available():
        return "cuda"
    return "cpu"


def load_rows(path: Path) -> list[dict]:
    rows: list[dict] = []
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            item = json.loads(line)
            messages = item.get("messages") or []
            if len(messages) < 2:
                continue
            rows.append({"messages": messages})
    if not rows:
        raise SystemExit(f"В {path} нет ни одной пары сообщений")
    return rows


def main() -> int:
    args = parse_args()

    try:
        import torch
        from datasets import Dataset
        from peft import LoraConfig, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            DataCollatorForLanguageModeling,
            Trainer,
            TrainingArguments,
        )
    except ImportError as exc:  # pragma: no cover - зависит от окружения
        print(f"Не хватает зависимости: {exc}", file=sys.stderr)
        print(
            "Установите: pip install torch transformers peft datasets\n"
            "Для Intel GPU: pip install torch --index-url https://download.pytorch.org/whl/xpu",
            file=sys.stderr,
        )
        return 2

    dataset_path = Path(args.dataset).expanduser()
    rows = load_rows(dataset_path)
    device = pick_device(args.device)
    print(f"Примеров: {len(rows)}; устройство: {device}; база: {args.base}")

    tokenizer = AutoTokenizer.from_pretrained(args.base, trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    model_kwargs: dict = {"trust_remote_code": True}
    if device == "cpu":
        model_kwargs["torch_dtype"] = torch.float32
    else:
        model_kwargs["torch_dtype"] = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16
    if args.load_4bit:
        from transformers import BitsAndBytesConfig

        model_kwargs["quantization_config"] = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_compute_dtype=torch.bfloat16,
            bnb_4bit_quant_type="nf4",
        )
        model_kwargs["device_map"] = "auto"

    model = AutoModelForCausalLM.from_pretrained(args.base, **model_kwargs)
    model.config.use_cache = False

    lora = LoraConfig(
        r=args.rank,
        lora_alpha=args.alpha,
        lora_dropout=args.dropout,
        bias="none",
        task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
    )
    model = get_peft_model(model, lora)
    model.print_trainable_parameters()

    def encode(example: dict) -> dict:
        text = tokenizer.apply_chat_template(
            example["messages"], tokenize=False, add_generation_prompt=False
        )
        tokens = tokenizer(
            text,
            truncation=True,
            max_length=args.max_len,
            padding=False,
            return_tensors=None,
        )
        tokens["labels"] = list(tokens["input_ids"])
        return tokens

    dataset = Dataset.from_list(rows).map(encode, remove_columns=["messages"])
    collator = DataCollatorForLanguageModeling(tokenizer=tokenizer, mlm=False)

    use_bf16 = device == "xpu" or (device == "cuda" and torch.cuda.is_bf16_supported())
    training_args = TrainingArguments(
        output_dir=args.output,
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        gradient_accumulation_steps=args.grad_accum,
        learning_rate=args.lr,
        logging_steps=5,
        save_steps=args.save_steps,
        save_total_limit=2,
        bf16=use_bf16,
        fp16=device == "cuda" and not use_bf16,
        report_to=[],
        remove_unused_columns=False,
        dataloader_pin_memory=False,
    )

    trainer = Trainer(
        model=model,
        args=training_args,
        train_dataset=dataset,
        data_collator=collator,
    )
    trainer.train()
    trainer.save_model(args.output)
    tokenizer.save_pretrained(args.output)
    print(f"Адаптер сохранён: {args.output}")
    print("Дальше: python scripts/export_openvino.py --base ... --adapter ... --output C:/Models/<name>-int4-ov")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
