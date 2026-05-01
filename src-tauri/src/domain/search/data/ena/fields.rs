use crate::domain::search::data::common::PublicDataSource;

pub(super) fn ena_fields(source: PublicDataSource) -> String {
    match source {
        PublicDataSource::Geo
        | PublicDataSource::CbioPortal
        | PublicDataSource::Gtex
        | PublicDataSource::NcbiDatasets => Vec::new(),
        PublicDataSource::EnaStudy => vec![
            "study_accession",
            "secondary_study_accession",
            "study_title",
            "description",
            "study_alias",
            "center_name",
            "tax_id",
            "scientific_name",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaRun => vec![
            "run_accession",
            "experiment_accession",
            "sample_accession",
            "study_accession",
            "secondary_study_accession",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
            "fastq_ftp",
            "fastq_md5",
            "submitted_ftp",
            "submitted_md5",
            "sra_ftp",
            "sra_md5",
        ],
        PublicDataSource::EnaExperiment => vec![
            "experiment_accession",
            "study_accession",
            "sample_accession",
            "experiment_title",
            "experiment_alias",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaSample => vec![
            "sample_accession",
            "secondary_sample_accession",
            "sample_alias",
            "scientific_name",
            "tax_id",
            "description",
            "country",
            "collection_date",
            "host",
            "host_tax_id",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaAnalysis => vec![
            "analysis_accession",
            "study_accession",
            "sample_accession",
            "analysis_title",
            "analysis_description",
            "analysis_alias",
            "analysis_type",
            "assembly_type",
            "description",
            "scientific_name",
            "first_public",
            "last_updated",
            "submitted_ftp",
            "submitted_md5",
            "generated_ftp",
            "generated_md5",
        ],
        PublicDataSource::EnaAssembly => vec![
            "assembly_accession",
            "scientific_name",
            "tax_id",
            "assembly_name",
            "assembly_title",
            "assembly_level",
            "description",
            "last_updated",
        ],
        PublicDataSource::EnaSequence => vec![
            "accession",
            "description",
            "scientific_name",
            "tax_id",
            "specimen_voucher",
            "bio_material",
            "first_public",
            "last_updated",
        ],
    }
    .join(",")
}

pub(super) fn ena_simple_search_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo
        | PublicDataSource::CbioPortal
        | PublicDataSource::Gtex
        | PublicDataSource::NcbiDatasets => &["description"],
        PublicDataSource::EnaStudy => &["study_title", "description"],
        PublicDataSource::EnaRun => &["description", "scientific_name", "study_title"],
        PublicDataSource::EnaExperiment => &["experiment_title", "description", "scientific_name"],
        PublicDataSource::EnaSample => &["description", "scientific_name", "sample_alias"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_description",
            "description",
            "analysis_type",
            "scientific_name",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "scientific_name",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}

pub(super) fn ena_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo
        | PublicDataSource::CbioPortal
        | PublicDataSource::Gtex
        | PublicDataSource::NcbiDatasets => &["accession"],
        PublicDataSource::EnaStudy => {
            &["study_accession", "secondary_study_accession", "accession"]
        }
        PublicDataSource::EnaRun => &["run_accession", "accession"],
        PublicDataSource::EnaExperiment => &["experiment_accession", "accession"],
        PublicDataSource::EnaSample => &[
            "sample_accession",
            "secondary_sample_accession",
            "accession",
        ],
        PublicDataSource::EnaAnalysis => &["analysis_accession", "accession"],
        PublicDataSource::EnaAssembly => &["assembly_accession", "accession"],
        PublicDataSource::EnaSequence => &["accession"],
    }
}

pub(super) fn ena_display_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::EnaStudy => {
            &["secondary_study_accession", "study_accession", "accession"]
        }
        PublicDataSource::EnaSample => &[
            "secondary_sample_accession",
            "sample_accession",
            "accession",
        ],
        _ => ena_accession_fields(source),
    }
}

pub(super) fn ena_title_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo
        | PublicDataSource::CbioPortal
        | PublicDataSource::Gtex
        | PublicDataSource::NcbiDatasets => &["title"],
        PublicDataSource::EnaStudy => &["study_title", "title", "description"],
        PublicDataSource::EnaRun => &["run_alias", "description", "run_accession"],
        PublicDataSource::EnaExperiment => &["experiment_title", "experiment_alias", "description"],
        PublicDataSource::EnaSample => &["sample_alias", "description", "scientific_name"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_alias",
            "analysis_description",
            "description",
            "analysis_accession",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "assembly_accession",
        ],
        PublicDataSource::EnaSequence => &["description", "accession"],
    }
}

pub(super) fn ena_summary_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo
        | PublicDataSource::CbioPortal
        | PublicDataSource::Gtex
        | PublicDataSource::NcbiDatasets => &["summary"],
        PublicDataSource::EnaStudy => &["study_description", "description"],
        PublicDataSource::EnaRun => &[
            "description",
            "library_strategy",
            "library_source",
            "instrument_model",
        ],
        PublicDataSource::EnaExperiment => &[
            "experiment_title",
            "experiment_alias",
            "library_strategy",
            "library_source",
        ],
        PublicDataSource::EnaSample => &["description", "sample_alias", "country"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_description",
            "analysis_title",
            "description",
            "analysis_type",
            "assembly_type",
        ],
        PublicDataSource::EnaAssembly => &[
            "description",
            "assembly_title",
            "assembly_name",
            "assembly_level",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}
