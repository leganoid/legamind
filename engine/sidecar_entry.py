"""Точка входа для PyInstaller-сборки сайдкара."""

from __future__ import annotations

import sys

from engine.__main__ import main


if __name__ == "__main__":
    sys.exit(main())
