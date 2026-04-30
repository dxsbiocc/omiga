//! Shared data-source types and serialization helpers.

mod parsing;
mod serialization;
mod types;

pub(in crate::domain::search::data) use parsing::{
    clean_html_text, clean_xml_fragment, encode_path_segment, extract_ena_file_links,
    extract_first_xml_tag, extract_xml_attr, json_number_string, json_string, json_u64_from_keys,
    json_u64_from_string_or_number, nested_string_field, normalize_accession, string_field_any,
    string_vec_field_any, truncate_for_error,
};
pub use serialization::{detail_to_json, search_response_to_json};
pub use types::{
    DataApiBaseUrls, DataRecord, DataSearchArgs, DataSearchResponse, PublicDataSource,
};
pub(in crate::domain::search::data) use types::{EntrezSettings, MAX_RESULTS_CAP};
