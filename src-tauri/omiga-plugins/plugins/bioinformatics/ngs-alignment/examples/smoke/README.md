# NGS Alignment smoke fixtures

Tiny FASTA/FASTQ/GTF inputs used to verify the wrapper contract without large
genome indexes or external network access. The automated smoke test uses mock
`bwa`, `bowtie2`, `STAR`, `hisat2`, and `samtools` executables so it validates
argument construction, output collection, manifests, and `outputs.json` only.
