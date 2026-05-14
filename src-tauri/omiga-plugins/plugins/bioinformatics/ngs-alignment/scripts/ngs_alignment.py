#!/usr/bin/env python3
"""Omiga NGS alignment operator implementations.

The operator executor prepares the declared conda envRef when micromamba,
mamba, or conda is available in the active PATH/base env/virtual env; these
wrappers then execute the selected aligner from that isolated environment.
"""
from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Iterable, Sequence


class OperatorError(RuntimeError):
    pass


def optional(value: str | None) -> str | None:
    if value is None:
        return None
    value = str(value).strip()
    if not value or value.lower() in {"none", "null"}:
        return None
    return value


def threads(value: str | int | None, default: int = 1) -> int:
    try:
        parsed = int(value) if value is not None else default
    except (TypeError, ValueError):
        parsed = default
    return max(1, parsed)


def split_extra(value: str | None) -> list[str]:
    text = optional(value)
    return shlex.split(text) if text else []


def ensure_tool(name: str) -> str:
    resolved = shutil.which(name)
    if not resolved:
        raise OperatorError(f"Required executable `{name}` was not found on PATH.")
    return resolved


def run(cmd: Sequence[str], *, stdout=None, stdin=None) -> None:
    print("[omiga-ngs]", shlex.join(str(part) for part in cmd), file=sys.stderr)
    subprocess.run([str(part) for part in cmd], check=True, stdout=stdout, stdin=stdin)


def run_pipe(producer: Sequence[str], consumer: Sequence[str]) -> None:
    print("[omiga-ngs]", shlex.join(str(part) for part in producer), "|", shlex.join(str(part) for part in consumer), file=sys.stderr)
    p1 = subprocess.Popen([str(part) for part in producer], stdout=subprocess.PIPE)
    assert p1.stdout is not None
    try:
        p2 = subprocess.run([str(part) for part in consumer], stdin=p1.stdout)
    finally:
        p1.stdout.close()
    p1_rc = p1.wait()
    if p1_rc != 0:
        raise subprocess.CalledProcessError(p1_rc, producer)
    if p2.returncode != 0:
        raise subprocess.CalledProcessError(p2.returncode, consumer)


def mkdir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def safe_prefix(prefix: str | None, fallback: str) -> str:
    raw = optional(prefix) or fallback
    cleaned = "".join(ch if ch.isalnum() or ch in {"_", "-", "."} else "_" for ch in raw)
    return cleaned.strip("._-") or fallback


def tar_dir(directory: Path, archive: Path) -> None:
    with tarfile.open(archive, "w:gz") as tar:
        for child in sorted(directory.rglob("*")):
            tar.add(child, arcname=child.relative_to(directory.parent))


def write_json(path: Path, payload: object) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_outputs(outdir: Path, summary: dict) -> None:
    write_json(outdir / "outputs.json", {"summary": summary})


def result_path(outdir: Path, prefix: str, suffix: str) -> Path:
    clean_prefix = safe_prefix(prefix, "samtools")
    suffix = suffix if suffix.startswith(".") else f".{suffix}"
    return outdir / f"samtools-{clean_prefix}{suffix}"


def write_index_manifest(outdir: Path, tool: str, index_dir: Path, prefix: str, archive: Path) -> None:
    files = [str(path.relative_to(index_dir.parent)) for path in sorted(index_dir.rglob("*")) if path.is_file()]
    write_json(
        outdir / "index-manifest.json",
        {
            "tool": tool,
            "indexDir": str(index_dir),
            "indexPrefix": str(index_dir / prefix),
            "archive": archive.name,
            "files": files,
        },
    )


def maybe_index_bam(bam: Path, sort_mode: str, make_index: bool) -> Path | None:
    if not make_index or sort_mode != "sorted_bam":
        return None
    ensure_tool("samtools")
    run(["samtools", "index", str(bam)])
    bai = Path(str(bam) + ".bai")
    return bai if bai.exists() else None


def pipe_sam_output(cmd: Sequence[str], outdir: Path, stem: str, output_mode: str, sort_order: str, make_index: bool, thread_count: int) -> tuple[Path, Path | None]:
    output_mode = output_mode or "sorted_bam"
    if output_mode == "sam":
        target = outdir / f"{stem}.sam"
        with target.open("wb") as handle:
            run(cmd, stdout=handle)
        return target, None

    ensure_tool("samtools")
    if output_mode == "bam":
        target = outdir / f"{stem}.bam"
        run_pipe(cmd, ["samtools", "view", "-@", str(max(1, thread_count - 1)), "-b", "-o", str(target), "-"])
        return target, None

    target = outdir / f"{stem}.bam"
    sort_cmd = ["samtools", "sort", "-@", str(max(1, thread_count - 1)), "-o", str(target)]
    if sort_order == "queryname":
        sort_cmd.insert(2, "-n")
    sort_cmd.append("-")
    run_pipe(cmd, sort_cmd)
    return target, maybe_index_bam(target, output_mode, make_index and sort_order == "coordinate")


def bwa_index(args: argparse.Namespace) -> None:
    ensure_tool("bwa")
    outdir = mkdir(Path(args.outdir))
    prefix = safe_prefix(args.prefix, "bwa-reference")
    index_dir = mkdir(outdir / "bwa-index")
    archive = outdir / "bwa-index.tar.gz"
    cmd = ["bwa", "index", "-p", str(index_dir / prefix)]
    algorithm = optional(args.algorithm)
    if algorithm == "auto":
        algorithm = None
    if algorithm:
        cmd.extend(["-a", algorithm])
    cmd.extend(split_extra(args.extra))
    cmd.append(args.reference)
    run(cmd)
    tar_dir(index_dir, archive)
    write_index_manifest(outdir, "bwa", index_dir, prefix, archive)
    write_outputs(outdir, {"tool": "bwa", "mode": "index", "indexPrefix": str(index_dir / prefix), "archive": archive.name})


def bwa_mem(args: argparse.Namespace) -> None:
    ensure_tool("bwa")
    outdir = mkdir(Path(args.outdir))
    thread_count = threads(args.threads)
    cmd = ["bwa", "mem", "-t", str(thread_count)]
    cmd.extend(split_extra(args.extra))
    cmd.extend([args.reference, args.reads1])
    if optional(args.reads2):
        cmd.append(args.reads2)
    alignment, alignment_index = pipe_sam_output(cmd, outdir, "bwa-aligned", args.output, args.sort_order, args.index_bam, thread_count)
    write_outputs(outdir, {"tool": "bwa", "mode": "mem", "alignment": alignment.name, "index": alignment_index.name if alignment_index else None})


def bowtie2_build(args: argparse.Namespace) -> None:
    ensure_tool("bowtie2-build")
    outdir = mkdir(Path(args.outdir))
    prefix = safe_prefix(args.prefix, "bowtie2-reference")
    index_dir = mkdir(outdir / "bowtie2-index")
    archive = outdir / "bowtie2-index.tar.gz"
    cmd = ["bowtie2-build", "--threads", str(threads(args.threads)), "-f"]
    cmd.extend(split_extra(args.extra))
    cmd.extend([args.reference, str(index_dir / prefix)])
    run(cmd)
    tar_dir(index_dir, archive)
    write_index_manifest(outdir, "bowtie2", index_dir, prefix, archive)
    write_outputs(outdir, {"tool": "bowtie2", "mode": "build", "indexPrefix": str(index_dir / prefix), "archive": archive.name})


def bowtie2_align(args: argparse.Namespace) -> None:
    ensure_tool("bowtie2")
    outdir = mkdir(Path(args.outdir))
    thread_count = threads(args.threads)
    cmd = ["bowtie2", "--threads", str(thread_count)]
    cmd.extend(split_extra(args.extra))
    cmd.extend(["-x", args.index_prefix])
    if optional(args.reads2):
        cmd.extend(["-1", args.reads1, "-2", args.reads2])
    else:
        cmd.extend(["-U", args.reads1])
    alignment, alignment_index = pipe_sam_output(cmd, outdir, "bowtie2-aligned", args.output, args.sort_order, args.index_bam, thread_count)
    write_outputs(outdir, {"tool": "bowtie2", "mode": "align", "alignment": alignment.name, "index": alignment_index.name if alignment_index else None})


def star_index(args: argparse.Namespace) -> None:
    ensure_tool("STAR")
    outdir = mkdir(Path(args.outdir))
    index_dir = mkdir(outdir / "star-index")
    archive = outdir / "star-index.tar.gz"
    with tempfile.TemporaryDirectory(prefix="omiga-star-index-") as tmp:
        cmd = [
            "STAR",
            "--runThreadN",
            str(threads(args.threads)),
            "--runMode",
            "genomeGenerate",
            "--genomeDir",
            str(index_dir),
            "--genomeFastaFiles",
            args.reference,
            "--outTmpDir",
            str(Path(tmp) / "STARtmp"),
        ]
        if optional(args.gtf):
            cmd.extend(["--sjdbGTFfile", args.gtf, "--sjdbOverhang", str(args.sjdb_overhang)])
        cmd.extend(split_extra(args.extra))
        run(cmd)
    tar_dir(index_dir, archive)
    write_index_manifest(outdir, "STAR", index_dir, "", archive)
    write_outputs(outdir, {"tool": "STAR", "mode": "genomeGenerate", "genomeDir": str(index_dir), "archive": archive.name})


def star_read_command(reads1: str, configured: str) -> list[str]:
    value = optional(configured) or "auto"
    if value == "none":
        return []
    if value == "auto":
        if reads1.endswith(".gz"):
            value = "gunzip -c"
        elif reads1.endswith(".bz2"):
            value = "bunzip2 -c"
        else:
            return []
    return ["--readFilesCommand", *shlex.split(value)]


def star_align(args: argparse.Namespace) -> None:
    ensure_tool("STAR")
    outdir = mkdir(Path(args.outdir))
    prefix = str(outdir / "star_")
    extra = split_extra(args.extra)
    with tempfile.TemporaryDirectory(prefix="omiga-star-align-") as tmp:
        cmd = [
            "STAR",
            "--runThreadN",
            str(threads(args.threads)),
            "--genomeDir",
            args.genome_dir,
            "--readFilesIn",
            args.reads1,
        ]
        if optional(args.reads2):
            cmd.append(args.reads2)
        cmd.extend(star_read_command(args.reads1, args.read_files_command))
        cmd.extend(["--outFileNamePrefix", prefix, "--outTmpDir", str(Path(tmp) / "STARtmp")])
        if "--outSAMtype" not in extra:
            cmd.extend(["--outSAMtype", "BAM", "SortedByCoordinate"])
        cmd.extend(extra)
        run(cmd)
    candidates = [
        outdir / "star_Aligned.sortedByCoord.out.bam",
        outdir / "star_Aligned.out.bam",
        outdir / "star_Aligned.out.sam",
    ]
    alignment = next((path for path in candidates if path.exists()), None)
    if alignment is None:
        matches = sorted(outdir.glob("star_Aligned*"))
        if not matches:
            raise OperatorError("STAR finished but no star_Aligned output was found.")
        alignment = matches[0]
    alignment_index = maybe_index_bam(alignment, "sorted_bam", args.index_bam and alignment.suffix == ".bam")
    write_outputs(outdir, {"tool": "STAR", "mode": "align", "alignment": alignment.name, "index": alignment_index.name if alignment_index else None})


def hisat2_index(args: argparse.Namespace) -> None:
    ensure_tool("hisat2-build")
    outdir = mkdir(Path(args.outdir))
    prefix = safe_prefix(args.prefix, "hisat2-reference")
    index_dir = mkdir(outdir / "hisat2-index")
    archive = outdir / "hisat2-index.tar.gz"
    cmd = ["hisat2-build", "-p", str(threads(args.threads))]
    cmd.extend(split_extra(args.extra))
    cmd.extend([args.reference, str(index_dir / prefix)])
    run(cmd)
    tar_dir(index_dir, archive)
    write_index_manifest(outdir, "hisat2", index_dir, prefix, archive)
    write_outputs(outdir, {"tool": "hisat2", "mode": "build", "indexPrefix": str(index_dir / prefix), "archive": archive.name})


def hisat2_align(args: argparse.Namespace) -> None:
    ensure_tool("hisat2")
    outdir = mkdir(Path(args.outdir))
    thread_count = threads(args.threads)
    cmd = ["hisat2", "-p", str(thread_count)]
    cmd.extend(split_extra(args.extra))
    cmd.extend(["-x", args.index_prefix])
    if optional(args.reads2):
        cmd.extend(["-1", args.reads1, "-2", args.reads2])
    else:
        cmd.extend(["-U", args.reads1])
    alignment, alignment_index = pipe_sam_output(cmd, outdir, "hisat2-aligned", args.output, args.sort_order, args.index_bam, thread_count)
    write_outputs(outdir, {"tool": "hisat2", "mode": "align", "alignment": alignment.name, "index": alignment_index.name if alignment_index else None})


def samtools_output_format(value: str | None, default: str = "bam") -> tuple[str, str, list[str]]:
    fmt = (optional(value) or "auto").lower()
    if fmt == "auto":
        fmt = default
    if fmt == "sam":
        return "sam", ".sam", []
    if fmt == "bam":
        return "bam", ".bam", ["-b"]
    if fmt == "cram":
        return "cram", ".cram", ["-C"]
    raise OperatorError(f"Unsupported samtools output format `{value}`.")


def samtools_utility(args: argparse.Namespace) -> None:
    ensure_tool("samtools")
    outdir = mkdir(Path(args.outdir))
    operation = (optional(args.operation) or "flagstat").lower().replace("-", "_")
    thread_count = threads(args.threads)
    extra = split_extra(args.extra)
    reference = optional(args.reference)
    region = optional(args.region)
    summary: dict[str, object]

    if operation == "view":
        fmt, suffix, format_args = samtools_output_format(args.output_format, "bam")
        target = result_path(outdir, args.output_prefix, suffix)
        cmd = ["samtools", "view", "-@", str(thread_count), *format_args, "-o", str(target)]
        if fmt == "cram" and reference:
            cmd.extend(["-T", reference])
        cmd.extend(extra)
        cmd.append(args.input)
        if region:
            cmd.append(region)
        run(cmd)
        summary = {
            "tool": "samtools",
            "operation": "view",
            "result": target.name,
            "format": fmt,
            "region": region,
        }
    elif operation == "sort":
        fmt, suffix, _format_args = samtools_output_format(args.output_format, "bam")
        target = result_path(outdir, args.output_prefix, suffix)
        cmd = ["samtools", "sort", "-@", str(thread_count), "-o", str(target)]
        if args.sort_order == "queryname":
            cmd.append("-n")
        if fmt != "bam":
            cmd.extend(["-O", fmt.upper()])
        if fmt == "cram" and reference:
            cmd.extend(["--reference", reference])
        cmd.extend(extra)
        cmd.append(args.input)
        run(cmd)
        summary = {
            "tool": "samtools",
            "operation": "sort",
            "result": target.name,
            "format": fmt,
            "sortOrder": args.sort_order,
        }
    elif operation == "index":
        suffix = ".crai" if str(args.input).lower().endswith(".cram") else ".bai"
        target = result_path(outdir, args.output_prefix, suffix)
        cmd = ["samtools", "index", "-@", str(thread_count), *extra, args.input, str(target)]
        run(cmd)
        summary = {"tool": "samtools", "operation": "index", "result": target.name}
    elif operation in {"flagstat", "stats", "idxstats"}:
        suffix = {
            "flagstat": ".flagstat.txt",
            "stats": ".stats.txt",
            "idxstats": ".idxstats.tsv",
        }[operation]
        target = result_path(outdir, args.output_prefix, suffix)
        cmd = ["samtools", operation]
        if operation in {"flagstat", "stats"}:
            cmd.extend(["-@", str(thread_count)])
        cmd.extend(extra)
        cmd.append(args.input)
        with target.open("wb") as handle:
            run(cmd, stdout=handle)
        summary = {"tool": "samtools", "operation": operation, "result": target.name}
    elif operation == "quickcheck":
        target = result_path(outdir, args.output_prefix, ".quickcheck.txt")
        cmd = ["samtools", "quickcheck", "-v", *extra, args.input]
        with target.open("wb") as handle:
            run(cmd, stdout=handle)
        if target.stat().st_size == 0:
            target.write_text(f"OK\t{args.input}\n", encoding="utf-8")
        summary = {"tool": "samtools", "operation": "quickcheck", "result": target.name}
    else:
        raise OperatorError(f"Unsupported samtools operation `{args.operation}`.")

    write_outputs(outdir, summary)


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Run an Omiga NGS alignment unit")
    sub = root.add_subparsers(dest="command", required=True)

    p = sub.add_parser("bwa-index")
    p.add_argument("reference")
    p.add_argument("outdir")
    p.add_argument("prefix")
    p.add_argument("algorithm")
    p.add_argument("extra")
    p.set_defaults(func=bwa_index)

    p = sub.add_parser("bwa-mem")
    p.add_argument("reference")
    p.add_argument("reads1")
    p.add_argument("reads2")
    p.add_argument("outdir")
    p.add_argument("threads")
    p.add_argument("output")
    p.add_argument("sort_order")
    p.add_argument("index_bam", type=lambda value: str(value).lower() == "true")
    p.add_argument("extra")
    p.set_defaults(func=bwa_mem)

    p = sub.add_parser("bowtie2-build")
    p.add_argument("reference")
    p.add_argument("outdir")
    p.add_argument("prefix")
    p.add_argument("threads")
    p.add_argument("extra")
    p.set_defaults(func=bowtie2_build)

    p = sub.add_parser("bowtie2-align")
    p.add_argument("index_prefix")
    p.add_argument("reads1")
    p.add_argument("reads2")
    p.add_argument("outdir")
    p.add_argument("threads")
    p.add_argument("output")
    p.add_argument("sort_order")
    p.add_argument("index_bam", type=lambda value: str(value).lower() == "true")
    p.add_argument("extra")
    p.set_defaults(func=bowtie2_align)

    p = sub.add_parser("star-index")
    p.add_argument("reference")
    p.add_argument("gtf")
    p.add_argument("outdir")
    p.add_argument("threads")
    p.add_argument("sjdb_overhang")
    p.add_argument("extra")
    p.set_defaults(func=star_index)

    p = sub.add_parser("star-align")
    p.add_argument("genome_dir")
    p.add_argument("reads1")
    p.add_argument("reads2")
    p.add_argument("outdir")
    p.add_argument("threads")
    p.add_argument("read_files_command")
    p.add_argument("index_bam", type=lambda value: str(value).lower() == "true")
    p.add_argument("extra")
    p.set_defaults(func=star_align)

    p = sub.add_parser("hisat2-index")
    p.add_argument("reference")
    p.add_argument("outdir")
    p.add_argument("prefix")
    p.add_argument("threads")
    p.add_argument("extra")
    p.set_defaults(func=hisat2_index)

    p = sub.add_parser("hisat2-align")
    p.add_argument("index_prefix")
    p.add_argument("reads1")
    p.add_argument("reads2")
    p.add_argument("outdir")
    p.add_argument("threads")
    p.add_argument("output")
    p.add_argument("sort_order")
    p.add_argument("index_bam", type=lambda value: str(value).lower() == "true")
    p.add_argument("extra")
    p.set_defaults(func=hisat2_align)

    p = sub.add_parser("samtools")
    p.add_argument("operation")
    p.add_argument("input")
    p.add_argument("outdir")
    p.add_argument("output_prefix")
    p.add_argument("threads")
    p.add_argument("output_format")
    p.add_argument("sort_order")
    p.add_argument("region")
    p.add_argument("reference")
    p.add_argument("extra")
    p.set_defaults(func=samtools_utility)
    return root


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        args.func(args)
    except subprocess.CalledProcessError as err:
        print(f"Command failed with exit code {err.returncode}: {err.cmd}", file=sys.stderr)
        return err.returncode or 1
    except OperatorError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
