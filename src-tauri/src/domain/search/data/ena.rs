//! ENA Portal and Browser API adapter.

mod fields;
mod operations;
mod parser;
mod query;

pub fn looks_like_ena_accession(value: &str) -> bool {
    query::looks_like_ena_accession(value)
}

pub fn inferred_ena_source_key(value: &str) -> Option<&'static str> {
    query::inferred_ena_source_key(value)
}

#[cfg(test)]
mod tests {
    use super::super::common::PublicDataSource;
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_ena_portal_json() {
        let value = json!([{
            "study_accession": "PRJEB123",
            "secondary_study_accession": "ERP123",
            "study_title": "Metagenome study",
            "description": "Rumen samples",
            "center_name": "EBI",
            "scientific_name": "cow metagenome",
            "first_public": "2024-01-01",
            "last_updated": "2024-02-01"
        }]);
        let records = parser::parse_ena_portal_json(PublicDataSource::EnaStudy, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "PRJEB123");
        assert_eq!(records[0].accession, "ERP123");
        assert_eq!(records[0].organism.as_deref(), Some("cow metagenome"));
        assert!(records[0].url.ends_with("/PRJEB123"));
    }

    #[test]
    fn parses_ena_run_portal_json_with_file_links() {
        let value = json!([{
            "run_accession": "ERR123",
            "experiment_accession": "ERX123",
            "sample_accession": "ERS123",
            "study_accession": "PRJEB123",
            "scientific_name": "Homo sapiens",
            "instrument_platform": "ILLUMINA",
            "instrument_model": "Illumina NovaSeq 6000",
            "library_strategy": "RNA-Seq",
            "fastq_ftp": "ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_1.fastq.gz;ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_2.fastq.gz"
        }]);
        let records = parser::parse_ena_portal_json(PublicDataSource::EnaRun, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "ERR123");
        assert_eq!(records[0].source, PublicDataSource::EnaRun);
        assert_eq!(records[0].platform.as_deref(), Some("ILLUMINA"));
        assert_eq!(records[0].files.len(), 2);
        assert_eq!(
            records[0].extra["library_strategy"].as_str(),
            Some("RNA-Seq")
        );
    }

    #[test]
    fn parses_ena_xml_record() {
        let xml = r#"
        <STUDY_SET>
          <STUDY accession="PRJEB999" alias="alias-1" center_name="EBI">
            <DESCRIPTOR>
              <STUDY_TITLE>XML Study</STUDY_TITLE>
              <STUDY_ABSTRACT>XML abstract &amp; details.</STUDY_ABSTRACT>
            </DESCRIPTOR>
            <STUDY_LINKS>
              <STUDY_LINK>
                <XREF_LINK><DB>ENA-FASTQ-FILES</DB><URL>ftp://example/file.fastq.gz</URL></XREF_LINK>
              </STUDY_LINK>
            </STUDY_LINKS>
          </STUDY>
        </STUDY_SET>
        "#;
        let record =
            parser::parse_ena_xml_record(PublicDataSource::EnaStudy, xml, "fallback").unwrap();
        assert_eq!(record.accession, "PRJEB999");
        assert_eq!(record.title, "XML Study");
        assert_eq!(record.summary, "XML abstract & details.");
        assert_eq!(record.files, vec!["ftp://example/file.fastq.gz"]);
    }

    #[test]
    fn builds_ena_queries_and_detects_record_types() {
        assert_eq!(
            PublicDataSource::parse("ena_run"),
            Some(PublicDataSource::EnaRun)
        );
        assert_eq!(
            PublicDataSource::parse("read_experiment"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(PublicDataSource::EnaRun.ena_result(), Some("read_run"));
        assert_eq!(
            query::ena_portal_query(PublicDataSource::EnaStudy, "rumen"),
            "study_title=\"*rumen*\" OR description=\"*rumen*\""
        );
        assert_eq!(
            query::ena_portal_query(PublicDataSource::EnaRun, "rumen"),
            "description=\"*rumen*\" OR scientific_name=\"*rumen*\" OR study_title=\"*rumen*\""
        );
        assert_eq!(
            query::ena_portal_query(
                PublicDataSource::EnaRun,
                "country=\"United Kingdom\" AND host_tax_id=9913"
            ),
            "country=\"United Kingdom\" AND host_tax_id=9913"
        );
        assert_eq!(
            query::ena_accession_query(PublicDataSource::EnaRun, "ERR123"),
            "run_accession=\"ERR123\""
        );
        assert_eq!(
            query::ena_accession_query(PublicDataSource::EnaRun, "ERX123"),
            "experiment_accession=\"ERX123\""
        );
        assert_eq!(
            query::ena_accession_query(PublicDataSource::EnaRun, "PRJEB123"),
            "study_accession=\"PRJEB123\" OR secondary_study_accession=\"PRJEB123\""
        );
        assert_eq!(
            query::ena_accession_query(PublicDataSource::EnaAnalysis, "SAMEA123"),
            "sample_accession=\"SAMEA123\" OR secondary_sample_accession=\"SAMEA123\""
        );
        assert_eq!(
            query::infer_ena_source_from_accession("ERX123"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(
            query::infer_ena_source_from_accession("ERZ123"),
            Some(PublicDataSource::EnaAnalysis)
        );
        let study_fields = fields::ena_fields(PublicDataSource::EnaStudy);
        assert!(study_fields.contains("description"));
        assert!(!study_fields.contains("study_description"));
        let assembly_fields = fields::ena_fields(PublicDataSource::EnaAssembly);
        assert!(assembly_fields.contains("assembly_title"));
        assert!(!assembly_fields.contains("first_public"));
        assert!(fields::ena_fields(PublicDataSource::EnaAnalysis).contains("generated_ftp"));
    }
}
