"""Слияние LoRA-адаптера с базовой моделью и экспорт в OpenVINO IR (int4).

Пример:
    python scripts/export_openvino.py --base Qwen/Qwen2.5-1.5B-Instruct \
        --adapter adapters/qwen-1.5b-lora --output C:/Models/qwen-1.5b-lora-int4-ov

Требует: torch, transformers, peft, optimum-intel (и nncf для int4).
Результат — папка, которую Legamind понимает как локальную модель
(openvino_model.xml/.bin + токенизаторы + config.json).
"""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Экспорт LoRA в OpenVINO IR")
    parser.add_argument("--base", required=True, help="HF id или путь к базовым весам")
    parser.add_argument("--adapter", required=True, help="папка с LoRA-адаптером")
    parser.add_argument("--output", required=True, help="папка для IR-модели")
    parser.add_argument("--weight-format", default="int4", choices=["int4", "int8", "fp16"])
    parser.add_argument("--merged-dir", default=None, help="куда сохранить слитые HF-веса (по умолчанию temp)")
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        import torch
        from peft import PeftModel
        from transformers import AutoModelForCausalLM, AutoTokenizer
    except ImportError as exc:  # pragma: no cover
        print(f"Не хватает зависимости: {exc}", file=sys.stderr)
        print("Установите: pip install torch transformers peft optimum-intel nncf", file=sys.stderr)
        return 2

    try:
        from optimum.intel import OVModelForCausalLM
    except ImportError as exc:  # pragma: no cover
        print(f"Нет optimum-intel: {exc}", file=sys.stderr)
        print("Установите: pip install optimum-intel", file=sys.stderr)
        return 2

    output = Path(args.output).expanduser()
    merged = Path(args.merged_dir).expanduser() if args.merged_dir else output.with_name(output.name + "-hf")
    merged.mkdir(parents=True, exist_ok=True)

    print(f"Загружаю базу {args.base}")
    base = AutoModelForCausalLM.from_pretrained(args.base, torch_dtype=torch.float32, trust_remote_code=True)
    tokenizer = AutoTokenizer.from_pretrained(args.base, trust_remote_code=True)

    print(f"Применяю адаптер {args.adapter}")
    model = PeftModel.from_pretrained(base, args.adapter)
    model = model.merge_and_unload()
    model.save_pretrained(merged, safe_serialization=True)
    tokenizer.save_pretrained(merged)
    print(f"Слитые веса: {merged}")

    print(f"Экспорт в OpenVINO ({args.weight_format}) → {output}")
    ov_model = OVModelForCausalLM.from_pretrained(
        str(merged),
        export=True,
        compile=False,
        ov_config={"weight_format": args.weight_format},
    )
    ov_model.save_pretrained(str(output))
    tokenizer.save_pretrained(str(output))

    # Копируем то, что нужно движку Legamind (config.json и токенизаторы уже на месте).
    for extra in ["generation_config.json", "special_tokens_map.json", "added_tokens.json"]:
        source = merged / extra
        if source.exists():
            shutil.copy2(source, output / extra)

    print("Готово.")
    print("1) Проверьте, что в папке есть openvino_model.xml и openvino_model.bin")
    print("2) На экране «Модели» добавьте путь к папке (или впишите в engine_config.json)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
