from pathlib import Path

from PyInstaller.__main__ import run

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target" / "sidecar"


def main() -> None:
    run(
        [
            "--noconfirm",
            "--clean",
            "--onedir",
            "--name",
            "sidecar",
            "--distpath",
            str(TARGET),
            "--workpath",
            str(TARGET / ".work"),
            "--specpath",
            str(TARGET / ".spec"),
            "--paths",
            str(ROOT / "python"),
            "--paths",
            str(ROOT / "python" / "lma_stt"),
            "--paths",
            str(ROOT / "python" / "lma_pipeline"),
            "--add-data",
            f"{ROOT / 'contracts'}:contracts",
            "--add-data",
            f"{ROOT / 'python' / 'sidecar' / 'storage' / 'migrations'}:sidecar/storage/migrations",
            "--collect-all",
            "sqlite_vec",
            str(ROOT / "python" / "sidecar" / "__main__.py"),
        ]
    )


if __name__ == "__main__":
    main()
