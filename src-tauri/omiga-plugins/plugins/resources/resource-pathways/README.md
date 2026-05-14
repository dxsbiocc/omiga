# Pathway Databases retrieval plugin

Provides Omiga Search / Query / Fetch routes for Reactome, Gene Ontology (QuickGO), MSigDB, and KEGG.

- `knowledge.reactome`: pathway search, detail fetch, and optional identifier overrepresentation analysis.
- `knowledge.gene_ontology`: GO term search/detail via EMBL-EBI QuickGO.
- `knowledge.msigdb`: MSigDB gene-set search/detail, including Hallmark, GO, Reactome, and KEGG-derived sets.
- `knowledge.kegg`: KEGG REST list/find/get/link/conv workflows. KEGG API use is intended for academic users; respect KEGG licensing.

The KEGG route follows the REST operations described by the referenced `scientific/kegg-database` skill: info, list, find, get, conv, link, and DDI-style endpoint structure where relevant.
