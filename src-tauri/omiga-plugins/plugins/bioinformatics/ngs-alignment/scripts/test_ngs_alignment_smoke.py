#!/usr/bin/env python3
"""Smoke-test the NGS alignment wrapper with mocked aligner binaries.

The goal is contract coverage, not biological correctness: each supported
subcommand is executed against tiny bundled fixtures while PATH points at mock
`bwa`, `bowtie2`, `STAR`, `hisat2`, and `samtools` executables. This keeps the
test deterministic and independent of conda/Docker/Singularity availability.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


PLUGIN_ROOT = Path(__file__).resolve().parents[1]
WRAPPER = PLUGIN_ROOT / "scripts" / "ngs_alignment.py"
FIXTURES = PLUGIN_ROOT / "examples" / "smoke"


FAKE_TOOL = r'''#!/usr/bin/env python3
from pathlib import Path
import sys

name = Path(sys.argv[0]).name
args = sys.argv[1:]

def after(flag, default=None):
    if flag not in args:
        return default
    idx = args.index(flag)
    return args[idx + 1] if idx + 1 < len(args) else default

def write(path, text):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")

def sam():
    sys.stdout.write("@HD\tVN:1.6\tSO:unknown\n")
    sys.stdout.write("smoke-read-1\t0\tchrSmoke\t1\t60\t8M\t*\t0\t0\tACGTACGT\tFFFFFFFF\n")

if name == "bwa":
    if args and args[0] == "index":
        prefix = after("-p")
        if not prefix:
            raise SystemExit("missing bwa -p prefix")
        for ext in [".amb", ".ann", ".bwt", ".pac", ".sa"]:
            write(prefix + ext, "bwa-index\n")
        raise SystemExit(0)
    if args and args[0] == "mem":
        sam()
        raise SystemExit(0)

if name == "bowtie2-build":
    prefix = args[-1]
    for ext in [".1.bt2", ".2.bt2", ".3.bt2", ".4.bt2", ".rev.1.bt2", ".rev.2.bt2"]:
        write(prefix + ext, "bowtie2-index\n")
    raise SystemExit(0)

if name == "bowtie2":
    sam()
    raise SystemExit(0)

if name == "hisat2-build":
    prefix = args[-1]
    for ext in [".1.ht2", ".2.ht2", ".3.ht2", ".4.ht2", ".5.ht2", ".6.ht2", ".7.ht2", ".8.ht2"]:
        write(prefix + ext, "hisat2-index\n")
    raise SystemExit(0)

if name == "hisat2":
    sam()
    raise SystemExit(0)

if name == "STAR":
    if "genomeGenerate" in args:
        genome_dir = after("--genomeDir")
        if not genome_dir:
            raise SystemExit("missing --genomeDir")
        write(Path(genome_dir) / "Genome", "star-genome\n")
        write(Path(genome_dir) / "SA", "star-sa\n")
        raise SystemExit(0)
    prefix = after("--outFileNamePrefix", "star_")
    write(prefix + "Aligned.sortedByCoord.out.bam", "star-bam\n")
    write(prefix + "Log.final.out", "star-log\n")
    raise SystemExit(0)

if name == "samtools":
    if not args:
        raise SystemExit("missing samtools subcommand")
    sub = args[0]
    if sub == "sort":
        output = after("-o")
        if not output:
            raise SystemExit("missing samtools sort -o")
        payload = sys.stdin.buffer.read() or b"bam\n"
        Path(output).parent.mkdir(parents=True, exist_ok=True)
        Path(output).write_bytes(payload)
        raise SystemExit(0)
    if sub == "view":
        output = after("-o")
        if not output:
            raise SystemExit("missing samtools view -o")
        payload = sys.stdin.buffer.read() or b"bam\n"
        Path(output).parent.mkdir(parents=True, exist_ok=True)
        Path(output).write_bytes(payload)
        raise SystemExit(0)
    if sub == "index":
        positional = [arg for arg in args[1:] if not arg.startswith("-") and arg.isdigit() is False]
        if not positional:
            raise SystemExit("missing bam path")
        target = positional[1] if len(positional) > 1 else positional[0] + ".bai"
        write(target, "bai\n")
        raise SystemExit(0)
    if sub == "flagstat":
        sys.stdout.write("1 + 0 in total (QC-passed reads + QC-failed reads)\n")
        raise SystemExit(0)
    if sub == "stats":
        sys.stdout.write("SN\traw total sequences:\t1\n")
        raise SystemExit(0)
    if sub == "idxstats":
        sys.stdout.write("chrSmoke\t8\t1\t0\n")
        raise SystemExit(0)
    if sub == "quickcheck":
        raise SystemExit(0)

raise SystemExit(f"unhandled fake tool invocation: {name} {args}")
'''


def install_fake_tools(mockbin: Path) -> None:
    mockbin.mkdir(parents=True, exist_ok=True)
    for name in [
        "bwa",
        "bowtie2-build",
        "bowtie2",
        "STAR",
        "hisat2-build",
        "hisat2",
        "samtools",
    ]:
        tool = mockbin / name
        tool.write_text(FAKE_TOOL, encoding="utf-8")
        tool.chmod(0o755)


def run_wrapper(env: dict[str, str], *args: str) -> None:
    subprocess.run(
        [sys.executable, str(WRAPPER), *map(str, args)],
        cwd=PLUGIN_ROOT,
        env=env,
        check=True,
    )


def read_summary(outdir: Path) -> dict:
    path = outdir / "outputs.json"
    assert path.is_file(), f"missing {path}"
    return json.loads(path.read_text(encoding="utf-8"))["summary"]


def assert_index_outputs(outdir: Path, archive: str, tool: str) -> None:
    assert (outdir / archive).is_file(), f"missing {archive}"
    manifest = outdir / "index-manifest.json"
    assert manifest.is_file(), f"missing {manifest}"
    data = json.loads(manifest.read_text(encoding="utf-8"))
    assert data["tool"] == tool
    assert data["files"], f"empty index manifest for {tool}"


def assert_alignment_summary(outdir: Path, expected_tool: str) -> None:
    summary = read_summary(outdir)
    assert summary["tool"] == expected_tool
    alignment = outdir / summary["alignment"]
    assert alignment.is_file(), f"missing alignment {alignment}"


def main() -> int:
    reference = FIXTURES / "reference.fa"
    reads = FIXTURES / "reads.fastq"
    annotation = FIXTURES / "annotation.gtf"
    assert reference.is_file()
    assert reads.is_file()
    assert annotation.is_file()

    with tempfile.TemporaryDirectory(prefix="omiga-ngs-smoke-") as tmp_raw:
        tmp = Path(tmp_raw)
        mockbin = tmp / "mockbin"
        install_fake_tools(mockbin)
        env = os.environ.copy()
        env["PATH"] = f"{mockbin}{os.pathsep}{env.get('PATH', '')}"
        env["PYTHONDONTWRITEBYTECODE"] = "1"

        bwa_index_out = tmp / "bwa-index"
        run_wrapper(env, "bwa-index", reference, bwa_index_out, "reference", "auto", "")
        assert_index_outputs(bwa_index_out, "bwa-index.tar.gz", "bwa")
        assert read_summary(bwa_index_out)["archive"] == "bwa-index.tar.gz"

        bwa_mem_out = tmp / "bwa-mem"
        run_wrapper(env, "bwa-mem", reference, reads, "none", bwa_mem_out, "2", "sorted_bam", "coordinate", "true", "")
        assert_alignment_summary(bwa_mem_out, "bwa")
        assert (bwa_mem_out / "bwa-aligned.bam.bai").is_file()

        bowtie2_build_out = tmp / "bowtie2-build"
        run_wrapper(env, "bowtie2-build", reference, bowtie2_build_out, "reference", "2", "")
        assert_index_outputs(bowtie2_build_out, "bowtie2-index.tar.gz", "bowtie2")

        bowtie2_align_out = tmp / "bowtie2-align"
        run_wrapper(env, "bowtie2-align", "mock-index", reads, "none", bowtie2_align_out, "2", "sam", "coordinate", "false", "")
        assert_alignment_summary(bowtie2_align_out, "bowtie2")

        star_index_out = tmp / "star-index"
        run_wrapper(env, "star-index", reference, annotation, star_index_out, "2", "7", "")
        assert_index_outputs(star_index_out, "star-index.tar.gz", "STAR")

        star_align_out = tmp / "star-align"
        run_wrapper(env, "star-align", star_index_out / "star-index", reads, "none", star_align_out, "2", "none", "true", "")
        assert_alignment_summary(star_align_out, "STAR")
        assert (star_align_out / "star_Aligned.sortedByCoord.out.bam.bai").is_file()

        hisat2_index_out = tmp / "hisat2-index"
        run_wrapper(env, "hisat2-index", reference, hisat2_index_out, "reference", "2", "")
        assert_index_outputs(hisat2_index_out, "hisat2-index.tar.gz", "hisat2")

        hisat2_align_out = tmp / "hisat2-align"
        run_wrapper(env, "hisat2-align", "mock-index", reads, "none", hisat2_align_out, "2", "bam", "coordinate", "false", "")
        assert_alignment_summary(hisat2_align_out, "hisat2")

        samtools_flagstat_out = tmp / "samtools-flagstat"
        run_wrapper(env, "samtools", "flagstat", bwa_mem_out / "bwa-aligned.bam", samtools_flagstat_out, "qc", "2", "auto", "coordinate", "none", "none", "")
        samtools_summary = read_summary(samtools_flagstat_out)
        assert samtools_summary["tool"] == "samtools"
        assert samtools_summary["operation"] == "flagstat"
        assert (samtools_flagstat_out / "samtools-qc.flagstat.txt").is_file()

    print("ngs-alignment smoke fixture passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
