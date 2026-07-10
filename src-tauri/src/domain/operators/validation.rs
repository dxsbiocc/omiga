use super::*;

#[cfg(test)]
use crate::domain::tools::ToolSchema;

pub(crate) const OPERATOR_PREFLIGHT_MAX_QUESTIONS: usize = 4;
pub(crate) const OPERATOR_PREFLIGHT_MAX_OPTIONS: usize = 5;
pub(crate) const OPERATOR_PREFLIGHT_ASK_STATE: &str = "ask";
pub(crate) const OPERATOR_PREFLIGHT_METADATA_KEY: &str = "preflight";
pub(crate) const OPERATOR_PARAM_SOURCE_USER_PREFLIGHT: &str = "user_preflight";
pub(crate) const OPERATOR_PARAM_SOURCE_CALLER: &str = "caller";
pub(crate) const OPERATOR_PARAM_SOURCE_DEFAULT: &str = "default";
pub(crate) const OPERATOR_PARAM_SOURCE_SYSTEM: &str = "system";

#[cfg(test)]
pub fn operator_tool_schema(operator: ResolvedOperator) -> ToolSchema {
    let name = format!("{OPERATOR_TOOL_PREFIX}{}", operator.alias);
    let mut description = operator
        .spec
        .metadata
        .description
        .clone()
        .or_else(|| operator.spec.metadata.name.clone())
        .unwrap_or_else(|| {
            format!(
                "Run operator {}@{}",
                operator.spec.metadata.id, operator.spec.metadata.version
            )
        });
    if let Some(resource_note) = operator_resource_profile_description(&operator.spec) {
        description.push_str("\n\nResource note: ");
        description.push_str(&resource_note);
    }
    ToolSchema::new(
        name,
        description,
        operator_parameters_schema(&operator.spec),
    )
}

#[cfg(test)]
fn operator_resource_profile_description(spec: &OperatorSpec) -> Option<String> {
    let profile = spec.runtime.as_ref()?.get("resourceProfile")?.as_object()?;
    let tier = profile
        .get("tier")
        .and_then(JsonValue::as_str)
        .map(|value| value.trim().to_ascii_lowercase().replace('_', "-"))
        .filter(|value| !value.is_empty())?;
    if tier == "local-ok" {
        return None;
    }
    let label = match tier.as_str() {
        "hpc-required" => "HPC required",
        "hpc-recommended" | "server-recommended" => "HPC/server recommended",
        "heavy" => "resource-heavy",
        "local-warn" => "local warning",
        _ => tier.as_str(),
    };
    let mut parts = vec![label.to_string()];
    if let Some(cpu) = profile.get("recommendedCpu").and_then(JsonValue::as_u64) {
        parts.push(format!("{cpu} CPU recommended"));
    }
    if let Some(memory) = profile
        .get("recommendedMemoryGb")
        .and_then(JsonValue::as_u64)
    {
        parts.push(format!("{memory} GB RAM recommended"));
    }
    if let Some(disk) = profile.get("diskGb").and_then(JsonValue::as_u64) {
        parts.push(format!("{disk} GB disk"));
    }
    let mut out = parts.join("; ");
    if let Some(note) = profile
        .get("notes")
        .and_then(JsonValue::as_array)
        .and_then(|notes| notes.iter().find_map(JsonValue::as_str))
        .map(str::trim)
        .filter(|note| !note.is_empty())
    {
        out.push_str(". ");
        out.push_str(note);
    }
    out.push_str(" Prefer SSH/server/HPC execution for production-size inputs; local smoke fixtures are acceptable.");
    Some(out)
}

pub(crate) fn operator_parameters_schema(spec: &OperatorSpec) -> JsonValue {
    let mut properties = JsonMap::new();
    let preflight_questions = preflight_question_text_by_param(spec);
    let operation_names = operator_operation_names(spec);
    properties.insert(
        "inputs".to_string(),
        fields_object_schema(&spec.interface.inputs, true, None),
    );
    let mut params_schema =
        fields_object_schema(&spec.interface.params, true, Some(&preflight_questions));
    if operation_names.len() > 1 {
        add_operation_to_params_schema(&mut params_schema, &operation_names);
    }
    properties.insert("params".to_string(), params_schema);
    properties.insert(
        "resources".to_string(),
        resources_object_schema(&spec.resources),
    );
    let mut required = vec![JsonValue::String("inputs".to_string())];
    if has_caller_required_fields(&spec.interface.params, Some(&preflight_questions)) {
        required.push(JsonValue::String("params".to_string()));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

pub(crate) fn operator_operation_names(spec: &OperatorSpec) -> Vec<String> {
    let mut names = spec.operations.keys().cloned().collect::<Vec<_>>();
    if names.is_empty() && !spec.execution.argv.is_empty() {
        names.push("run".to_string());
    }
    names
}

pub(crate) fn add_operation_to_params_schema(
    params_schema: &mut JsonValue,
    operation_names: &[String],
) {
    let JsonValue::Object(params) = params_schema else {
        return;
    };
    let properties = params
        .entry("properties".to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if let JsonValue::Object(properties) = properties {
        properties.insert(
            "operation".to_string(),
            json!({
                "type": "string",
                "enum": operation_names,
                "description": "Operator operation/subcommand to run. Subcommands are operation parameters, not separate operator tools."
            }),
        );
    }
    let required = params
        .entry("required".to_string())
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    if let JsonValue::Array(required) = required {
        if !required
            .iter()
            .any(|item| item.as_str() == Some("operation"))
        {
            required.push(JsonValue::String("operation".to_string()));
        }
    }
}

pub(crate) fn list_operator_summaries_for_plugin_root(
    source_plugin: &str,
    plugin_root: &Path,
) -> Vec<OperatorCandidateSummary> {
    let mut candidates = discover_manifest_paths(plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            load_operator_manifest(
                &manifest_path,
                source_plugin.to_string(),
                plugin_root.to_path_buf(),
            )
            .ok()
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.metadata
            .id
            .cmp(&right.metadata.id)
            .then_with(|| left.metadata.version.cmp(&right.metadata.version))
            .then_with(|| left.source.source_plugin.cmp(&right.source.source_plugin))
    });
    candidates
        .into_iter()
        .map(|candidate| operator_candidate_summary(candidate, Vec::new()))
        .collect()
}

pub(crate) fn operator_operation_summaries_for_spec(
    spec: &OperatorSpec,
    exposed: bool,
) -> Vec<OperatorOperationSummary> {
    operator_operation_summaries(spec, exposed)
}

pub(crate) fn operator_operation_groups_for_spec(
    spec: &OperatorSpec,
) -> Vec<OperatorOperationGroupSummary> {
    let mut groups: BTreeMap<String, OperatorOperationGroupSummary> = BTreeMap::new();
    for (operation_id, operation) in &spec.operations {
        let (key, label) = operation_group_key(operation);
        let entry = groups
            .entry(key.clone())
            .or_insert_with(|| OperatorOperationGroupSummary {
                key,
                label,
                category: operation.category.clone(),
                group: operation.group.clone(),
                stage: operation.stage.clone(),
                operations: Vec::new(),
            });
        entry.operations.push(operation_id.clone());
    }
    groups.into_values().collect()
}

fn operation_group_key(operation: &OperatorOperationSpec) -> (String, String) {
    let label = operation
        .stage
        .as_ref()
        .or(operation.group.as_ref())
        .or(operation.category.as_ref())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Operations".to_string());
    let key = label
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '/' || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    (
        if key.is_empty() {
            "operations".to_string()
        } else {
            key
        },
        label,
    )
}

pub(crate) fn operator_spec_for_operation(
    spec: &OperatorSpec,
    operation_id: &str,
) -> Result<OperatorSpec, OperatorToolError> {
    let operation_id = operation_id.trim();
    if spec.operations.is_empty() {
        if operation_id.is_empty() || operation_id == "run" {
            return Ok(spec.clone());
        }
        return Err(OperatorToolError::new(
            "unknown_operation",
            false,
            format!(
                "Operator `{}` does not declare operation `{operation_id}`.",
                spec.metadata.id
            ),
        )
        .with_suggested_action(
            "Use operator_describe or unit_describe to inspect supported operations.",
        ));
    }
    let operation = spec.operations.get(operation_id).ok_or_else(|| {
        let supported = operator_operation_names(spec).join(", ");
        OperatorToolError::new(
            "unknown_operation",
            false,
            format!(
                "Operator `{}` does not declare operation `{operation_id}`. Supported operations: {supported}.",
                spec.metadata.id
            ),
        )
        .with_suggested_action("Retry with one of the operation enum values from operator_describe/unit_describe.")
    })?;
    let mut effective = spec.clone();
    if let Some(description) = operation.description.clone() {
        effective.metadata.description = Some(description);
    }
    effective.interface = operation.interface.clone();
    effective.smoke_tests = operation.smoke_tests.clone();
    effective.execution = operation.execution.clone();
    effective.preflight = operation.preflight.clone();
    effective.runtime = operation.runtime.clone();
    effective.cache = operation.cache.clone();
    effective.resources = operation.resources.clone();
    effective.bindings = operation.bindings.clone();
    effective.permissions = operation.permissions.clone();
    effective.operations = BTreeMap::from([(operation_id.to_string(), operation.clone())]);
    Ok(effective)
}

pub(crate) fn operator_operation_from_invocation(
    spec: &OperatorSpec,
    invocation: &OperatorInvocation,
) -> Result<String, OperatorToolError> {
    let operation_from_param = invocation
        .params
        .get("operation")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let operation = invocation
        .operation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(operation_from_param);
    let names = operator_operation_names(spec);
    match (operation, names.as_slice()) {
        (Some(operation), _) => Ok(operation),
        (None, [only]) => Ok(only.clone()),
        (None, []) => Ok("run".to_string()),
        (None, many) => Err(OperatorToolError::new(
            "missing_operation",
            false,
            format!(
                "Operator `{}` has multiple operations; choose one of: {}.",
                spec.metadata.id,
                many.join(", ")
            ),
        )
        .with_suggested_action(
            "Set `operation` on operator_execute, or `params.operation` for legacy compatibility.",
        )),
    }
}

pub(crate) fn preflight_question_text_by_param(spec: &OperatorSpec) -> BTreeMap<String, String> {
    spec.preflight
        .as_ref()
        .map(|preflight| {
            preflight
                .questions
                .iter()
                .map(|question| (question.param.clone(), question.question.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn has_caller_required_fields(
    fields: &BTreeMap<String, OperatorFieldSpec>,
    preflight_questions: Option<&BTreeMap<String, String>>,
) -> bool {
    fields.iter().any(|(name, field)| {
        field.required
            && field.default.is_none()
            && !preflight_questions
                .map(|questions| questions.contains_key(name))
                .unwrap_or(false)
    })
}

pub(crate) fn fields_object_schema(
    fields: &BTreeMap<String, OperatorFieldSpec>,
    include_required: bool,
    preflight_questions: Option<&BTreeMap<String, String>>,
) -> JsonValue {
    let mut properties = JsonMap::new();
    let mut required = Vec::new();
    for (name, field) in fields {
        let is_preflight_answered = preflight_questions
            .map(|questions| questions.contains_key(name))
            .unwrap_or(false);
        if include_required && field.required && field.default.is_none() && !is_preflight_answered {
            required.push(JsonValue::String(name.clone()));
        }
        properties.insert(
            name.clone(),
            field_schema(
                field,
                preflight_questions.and_then(|questions| questions.get(name)),
            ),
        );
    }
    let mut schema = JsonMap::new();
    schema.insert("type".to_string(), JsonValue::String("object".to_string()));
    schema.insert("properties".to_string(), JsonValue::Object(properties));
    schema.insert("additionalProperties".to_string(), JsonValue::Bool(false));
    if !required.is_empty() {
        schema.insert("required".to_string(), JsonValue::Array(required));
    }
    JsonValue::Object(schema)
}

pub(crate) fn field_schema(
    field: &OperatorFieldSpec,
    preflight_question: Option<&String>,
) -> JsonValue {
    let mut schema = JsonMap::new();
    match field.kind {
        OperatorFieldKind::Integer => {
            schema.insert("type".to_string(), JsonValue::String("integer".to_string()));
        }
        OperatorFieldKind::Number => {
            schema.insert("type".to_string(), JsonValue::String("number".to_string()));
        }
        OperatorFieldKind::Boolean => {
            schema.insert("type".to_string(), JsonValue::String("boolean".to_string()));
        }
        OperatorFieldKind::Json => {
            schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        }
        kind if kind.is_array() => {
            schema.insert("type".to_string(), JsonValue::String("array".to_string()));
            schema.insert("items".to_string(), json!({"type": "string"}));
        }
        _ => {
            schema.insert("type".to_string(), JsonValue::String("string".to_string()));
        }
    }
    if let Some(description) = field_description(field, preflight_question.map(String::as_str)) {
        schema.insert("description".to_string(), JsonValue::String(description));
    }
    if let Some(default) = &field.default {
        schema.insert("default".to_string(), default.clone());
    }
    if !field.enum_values.is_empty() {
        schema.insert(
            "enum".to_string(),
            JsonValue::Array(field.enum_values.clone()),
        );
    }
    if let Some(minimum) = field.minimum {
        schema.insert("minimum".to_string(), json!(minimum));
    }
    if let Some(maximum) = field.maximum {
        schema.insert("maximum".to_string(), json!(maximum));
    }
    let value_schema = JsonValue::Object(schema);
    if preflight_question.is_some() {
        return preflight_ask_state_schema(value_schema);
    }
    value_schema
}

pub(crate) fn preflight_ask_state_schema(value_schema: JsonValue) -> JsonValue {
    let mut wrapped = JsonMap::new();
    if let Some(description) = value_schema.get("description").cloned() {
        wrapped.insert("description".to_string(), description);
    }
    wrapped.insert(
        "oneOf".to_string(),
        json!([
            value_schema,
            {
                "type": "string",
                "enum": [OPERATOR_PREFLIGHT_ASK_STATE],
                "description": "Explicit ask state: set this parameter to `ask` to make Omiga ask the user through the operator preflight UI."
            }
        ]),
    );
    JsonValue::Object(wrapped)
}

pub(crate) fn field_description(
    field: &OperatorFieldSpec,
    preflight_question: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(description) = &field.description {
        parts.push(description.clone());
    }
    if let Some(question) = preflight_question {
        parts.push(format!(
            "Ask state: omit this value or set it to `{OPERATOR_PREFLIGHT_ASK_STATE}` to make Omiga collect it before execution (`{question}`); do not guess a value unless the user already specified it."
        ));
    }
    if field.kind.is_path_like() {
        parts.push(
            "Path string accepted; Omiga canonicalizes it to a FileRef/ArtifactRef.".to_string(),
        );
    }
    if !field.formats.is_empty() {
        parts.push(format!("Expected formats: {}.", field.formats.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn operator_preflight_question(
    tool_name: &str,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let resolved = resolve_operator_alias(tool_name).ok()?;
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let params = value.get("params").and_then(JsonValue::as_object);
    operator_preflight_question_for_spec(&resolved.spec, Some(resolved.alias.as_str()), params)
}

pub(crate) fn operator_preflight_question_with_project_preferences(
    project_root: &Path,
    tool_name: &str,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let resolved = resolve_operator_alias(tool_name).ok()?;
    let value = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let invocation = serde_json::from_str::<OperatorInvocation>(arguments).ok()?;
    let operation_id = operator_operation_from_invocation(&resolved.spec, &invocation).ok()?;
    let operation_spec = operator_spec_for_operation(&resolved.spec, &operation_id).ok()?;
    let params = value.get("params").and_then(JsonValue::as_object);
    let recommended_params = operator_project_preference_params(project_root, &operation_spec);
    operator_preflight_question_for_spec_with_recommended_params(
        &operation_spec,
        Some(resolved.alias.as_str()),
        params,
        recommended_params.as_ref(),
    )
}

pub(crate) fn operator_execute_preflight_question_with_project_preferences(
    project_root: &Path,
    arguments: &str,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let (alias, resolved, invocation, value) = operator_execute_parts(arguments).ok()?;
    let operation_id = operator_operation_from_invocation(&resolved.spec, &invocation).ok()?;
    let operation_spec = operator_spec_for_operation(&resolved.spec, &operation_id).ok()?;
    let params = value.get("params").and_then(JsonValue::as_object);
    let recommended_params = operator_project_preference_params(project_root, &operation_spec);
    operator_preflight_question_for_spec_with_recommended_params(
        &operation_spec,
        Some(alias.as_str()),
        params,
        recommended_params.as_ref(),
    )
}

pub fn operator_preflight_question_for_spec(
    spec: &OperatorSpec,
    alias: Option<&str>,
    params: Option<&JsonMap<String, JsonValue>>,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    operator_preflight_question_for_spec_with_recommended_params(spec, alias, params, None)
}

pub(crate) fn operator_preflight_question_for_spec_with_recommended_params(
    spec: &OperatorSpec,
    alias: Option<&str>,
    params: Option<&JsonMap<String, JsonValue>>,
    recommended_params: Option<&BTreeMap<String, JsonValue>>,
) -> Option<crate::domain::tools::ask_user_question::AskUserQuestionArgs> {
    let preflight = spec.preflight.as_ref()?;
    let recommended_keys = recommended_params
        .map(|params| params.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let questions = preflight
        .questions
        .iter()
        .filter(|question| preflight_question_should_ask(question, params))
        .map(
            |question| crate::domain::tools::ask_user_question::QuestionItem {
                question: question.question.clone(),
                header: question.header.clone(),
                multi_select: question.multi_select,
                param: Some(question.param.clone()),
                show_when: question.show_when.as_ref().map(|sw| {
                    crate::domain::tools::ask_user_question::QuestionShowWhen {
                        param: sw.param.clone(),
                        value: sw.value.clone(),
                    }
                }),
                options: ask_options_from_specs(
                    question,
                    recommended_params.and_then(|params| params.get(&question.param)),
                ),
            },
        )
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return None;
    }

    Some(
        crate::domain::tools::ask_user_question::AskUserQuestionArgs {
            questions,
            answers: None,
            annotations: None,
            metadata: Some(json!({
                "source": "operator_preflight",
                "operator_id": spec.metadata.id,
                "operator_alias": alias,
                "recommended_params": recommended_keys,
            })),
        },
    )
}

pub(crate) fn apply_operator_preflight_answers(
    tool_name: &str,
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let resolved = match resolve_operator_alias(tool_name) {
        Ok(resolved) => resolved,
        Err(_) => return Ok(arguments.to_string()),
    };
    let invocation = match serde_json::from_str::<OperatorInvocation>(arguments) {
        Ok(invocation) => invocation,
        Err(_) => return Ok(arguments.to_string()),
    };
    let operation_id = match operator_operation_from_invocation(&resolved.spec, &invocation) {
        Ok(operation_id) => operation_id,
        Err(_) => return Ok(arguments.to_string()),
    };
    let operation_spec = match operator_spec_for_operation(&resolved.spec, &operation_id) {
        Ok(spec) => spec,
        Err(_) => return Ok(arguments.to_string()),
    };
    let Some(preflight) = operation_spec.preflight.as_ref() else {
        return Ok(arguments.to_string());
    };
    apply_operator_preflight_answers_for_spec(
        &operation_spec,
        preflight,
        arguments,
        ask_user_output,
    )
}

pub(crate) fn apply_operator_execute_preflight_answers(
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let (_alias, resolved, invocation, _value) = match operator_execute_parts(arguments) {
        Ok(parts) => parts,
        Err(_) => return Ok(arguments.to_string()),
    };
    let operation_id = match operator_operation_from_invocation(&resolved.spec, &invocation) {
        Ok(operation_id) => operation_id,
        Err(_) => return Ok(arguments.to_string()),
    };
    let operation_spec = match operator_spec_for_operation(&resolved.spec, &operation_id) {
        Ok(spec) => spec,
        Err(_) => return Ok(arguments.to_string()),
    };
    let Some(preflight) = operation_spec.preflight.as_ref() else {
        return Ok(arguments.to_string());
    };
    apply_operator_preflight_answers_for_spec(
        &operation_spec,
        preflight,
        arguments,
        ask_user_output,
    )
}

pub(crate) fn operator_execute_parts(
    arguments: &str,
) -> Result<(String, ResolvedOperator, OperatorInvocation, JsonValue), String> {
    let value = serde_json::from_str::<JsonValue>(arguments)
        .map_err(|err| format!("Invalid operator_execute arguments JSON: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "operator_execute arguments must be an object".to_string())?;
    let alias = object
        .get("operator")
        .or_else(|| object.get("program"))
        .or_else(|| object.get("id"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "operator_execute requires operator/program/id".to_string())?
        .to_string();
    let resolved = resolve_operator_alias(&alias).map_err(|error| error.message)?;
    let invocation = OperatorInvocation {
        operation: object
            .get("operation")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        inputs: object_map_to_btree(object.get("inputs")),
        params: object_map_to_btree(object.get("params")),
        resources: object_map_to_btree(object.get("resources")),
        metadata: object_map_to_btree(object.get("metadata")),
    };
    Ok((alias, resolved, invocation, value))
}

pub(crate) fn object_map_to_btree(value: Option<&JsonValue>) -> BTreeMap<String, JsonValue> {
    value
        .and_then(JsonValue::as_object)
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn apply_operator_preflight_answers_for_spec(
    spec: &OperatorSpec,
    preflight: &OperatorPreflightSpec,
    arguments: &str,
    ask_user_output: &JsonValue,
) -> Result<String, String> {
    let answers = ask_user_output
        .get("answers")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "ask_user_question output did not contain an answers object".to_string())?;
    let mut invocation = serde_json::from_str::<JsonValue>(arguments)
        .map_err(|err| format!("Invalid operator arguments JSON: {err}"))?;
    let root = invocation
        .as_object_mut()
        .ok_or_else(|| "Operator arguments must be a JSON object".to_string())?;
    let mut answered_params = Vec::new();
    {
        let params_value = root
            .entry("params".to_string())
            .or_insert_with(|| JsonValue::Object(JsonMap::new()));
        if !params_value.is_object() {
            *params_value = JsonValue::Object(JsonMap::new());
        }
        let params = params_value
            .as_object_mut()
            .ok_or_else(|| "Operator params must be a JSON object".to_string())?;

        for question in &preflight.questions {
            let Some(answer) = answers.get(question.question.trim()) else {
                continue;
            };
            let labels = preflight_answer_labels(answer, question.multi_select);
            if labels.is_empty() {
                continue;
            }
            let field = spec
                .interface
                .params
                .get(question.param.trim())
                .ok_or_else(|| {
                    format!(
                        "Preflight question references unknown operator parameter `{}`",
                        question.param
                    )
                })?;
            let values = labels
                .iter()
                .map(|label| preflight_value_for_answer(question, field, label))
                .collect::<Result<Vec<_>, _>>()?;
            let value = if question.multi_select {
                JsonValue::Array(values)
            } else {
                values
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Missing preflight choice for `{}`", question.param))?
            };
            params.insert(question.param.clone(), value);
            answered_params.push(json!({
                "param": question.param.clone(),
                "questionId": question.id.clone(),
                "question": question.question.clone(),
                "labels": labels,
            }));
        }
    }
    if !answered_params.is_empty() {
        attach_operator_preflight_metadata(root, spec, answered_params);
    }

    serde_json::to_string(&invocation).map_err(|err| err.to_string())
}

pub(crate) fn attach_operator_preflight_metadata(
    root: &mut JsonMap<String, JsonValue>,
    spec: &OperatorSpec,
    answered_params: Vec<JsonValue>,
) {
    let mut params_by_source = JsonMap::new();
    for param in answered_params
        .iter()
        .filter_map(|entry| entry.get("param").and_then(JsonValue::as_str))
    {
        params_by_source.insert(
            param.to_string(),
            JsonValue::String(OPERATOR_PARAM_SOURCE_USER_PREFLIGHT.to_string()),
        );
    }
    let metadata_value = root
        .entry("metadata".to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !metadata_value.is_object() {
        *metadata_value = JsonValue::Object(JsonMap::new());
    }
    if let Some(metadata) = metadata_value.as_object_mut() {
        metadata.insert(
            OPERATOR_PREFLIGHT_METADATA_KEY.to_string(),
            json!({
                "source": "operator_preflight",
                "operatorId": spec.metadata.id,
                "answeredParams": answered_params,
                "paramsBySource": params_by_source,
            }),
        );
    }
}

pub(crate) fn preflight_question_should_ask(
    question: &OperatorPreflightQuestionSpec,
    params: Option<&JsonMap<String, JsonValue>>,
) -> bool {
    if question.ask_when.always {
        return true;
    }
    let value = params.and_then(|params| params.get(&question.param));
    if value
        .map(json_value_is_preflight_ask_state)
        .unwrap_or(false)
    {
        return true;
    }
    let missing = value.is_none() || matches!(value, Some(JsonValue::Null));
    if question.ask_when.missing && missing {
        return true;
    }
    if question.ask_when.empty && value.map(json_value_is_empty).unwrap_or(false) {
        return true;
    }
    if let Some(actual) = value {
        return question
            .ask_when
            .values
            .iter()
            .any(|expected| preflight_value_matches(actual, expected));
    }
    false
}

pub(crate) fn json_value_is_preflight_ask_state(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(value) => value
            .trim()
            .eq_ignore_ascii_case(OPERATOR_PREFLIGHT_ASK_STATE),
        JsonValue::Object(values) => {
            values
                .get("state")
                .or_else(|| values.get("status"))
                .and_then(JsonValue::as_str)
                .map(|value| {
                    value
                        .trim()
                        .eq_ignore_ascii_case(OPERATOR_PREFLIGHT_ASK_STATE)
                })
                .unwrap_or(false)
                || values
                    .get(OPERATOR_PREFLIGHT_ASK_STATE)
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

pub(crate) fn json_value_is_empty(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::String(value) => value.trim().is_empty(),
        JsonValue::Array(values) => values.is_empty(),
        JsonValue::Object(values) => values.is_empty(),
        _ => false,
    }
}

pub(crate) fn preflight_value_matches(actual: &JsonValue, expected: &JsonValue) -> bool {
    match (actual, expected) {
        (JsonValue::String(left), JsonValue::String(right)) => {
            left.trim().eq_ignore_ascii_case(right.trim())
        }
        _ => actual == expected,
    }
}

pub(crate) fn operator_project_preference_params(
    project_root: &Path,
    spec: &OperatorSpec,
) -> Option<BTreeMap<String, JsonValue>> {
    let canonical_id =
        crate::domain::operators::execution_types::canonical_operator_unit_id_for_spec(spec);
    let hints = crate::domain::learning_proposals::matching_learning_project_preference_hints(
        project_root,
        Some(spec.metadata.id.as_str()),
        Some(canonical_id.as_str()),
        Some(spec.source.source_plugin.as_str()),
    )
    .ok()?;
    let mut params = BTreeMap::new();
    for hint in hints.hints {
        for (key, value) in hint.params {
            params.entry(key).or_insert(value);
        }
    }
    (!params.is_empty()).then_some(params)
}

pub(crate) fn ask_options_from_specs(
    question: &OperatorPreflightQuestionSpec,
    recommended_value: Option<&JsonValue>,
) -> Vec<crate::domain::tools::ask_user_question::QuestionOption> {
    let recommended_index = recommended_value.and_then(|value| {
        question
            .options
            .iter()
            .position(|option| preflight_option_matches_recommended_value(option, value))
    });
    let mut options = question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| ask_option_from_spec(option, Some(index) == recommended_index))
        .collect::<Vec<_>>();
    if let Some(index) = recommended_index.filter(|index| *index < options.len()) {
        let recommended = options.remove(index);
        options.insert(0, recommended);
    }
    options
}

pub(crate) fn preflight_option_matches_recommended_value(
    option: &OperatorPreflightOptionSpec,
    recommended_value: &JsonValue,
) -> bool {
    preflight_value_matches(&option.value, recommended_value)
        || recommended_value
            .as_str()
            .map(|value| option.label.trim().eq_ignore_ascii_case(value.trim()))
            .unwrap_or(false)
}

pub(crate) fn ask_option_from_spec(
    option: &OperatorPreflightOptionSpec,
    recommended: bool,
) -> crate::domain::tools::ask_user_question::QuestionOption {
    let description = if recommended {
        format!(
            "推荐：可直接确认，或改选其他选项。{}",
            option.description.trim()
        )
    } else {
        option.description.clone()
    };
    crate::domain::tools::ask_user_question::QuestionOption {
        label: option.label.clone(),
        description,
        preview: option.preview.clone(),
        recommended,
        custom: option.custom,
        custom_placeholder: option.custom_placeholder.clone(),
    }
}

pub(crate) fn preflight_value_for_answer(
    question: &OperatorPreflightQuestionSpec,
    field: &OperatorFieldSpec,
    answer_label: &str,
) -> Result<JsonValue, String> {
    if let Some(option) = question
        .options
        .iter()
        .find(|option| option.label.trim() == answer_label)
    {
        if option.custom {
            return Err(format!(
                "Custom preflight choice `{}` for operator parameter `{}` needs a value",
                option.label, question.param
            ));
        }
        return Ok(option.value.clone());
    }

    for option in question.options.iter().filter(|option| option.custom) {
        if let Some(raw_value) = strip_custom_answer_value(answer_label, option.label.trim()) {
            return parse_custom_preflight_value(field, raw_value).map_err(|err| {
                format!(
                    "Invalid custom value for operator parameter `{}`: {err}",
                    question.param
                )
            });
        }
    }

    Err(format!(
        "Unsupported preflight choice `{}` for operator parameter `{}`",
        answer_label, question.param
    ))
}

pub(crate) fn strip_custom_answer_value<'a>(answer: &'a str, label: &str) -> Option<&'a str> {
    let answer = answer.trim();
    let rest = answer.strip_prefix(label)?.trim_start();
    let rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix('：'))?
        .trim();
    (!rest.is_empty()).then_some(rest)
}

pub(crate) fn parse_custom_preflight_value(
    field: &OperatorFieldSpec,
    raw: &str,
) -> Result<JsonValue, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("value is empty".to_string());
    }
    let value = match field.kind {
        OperatorFieldKind::Integer => {
            json!(raw
                .parse::<i64>()
                .map_err(|_| "expected an integer".to_string())?)
        }
        OperatorFieldKind::Number => {
            let parsed = raw
                .parse::<f64>()
                .map_err(|_| "expected a number".to_string())?;
            if !parsed.is_finite() {
                return Err("expected a finite number".to_string());
            }
            json!(parsed)
        }
        OperatorFieldKind::Boolean => match raw.to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" | "是" => JsonValue::Bool(true),
            "false" | "no" | "n" | "0" | "否" => JsonValue::Bool(false),
            _ => return Err("expected true/false".to_string()),
        },
        OperatorFieldKind::Json => {
            serde_json::from_str(raw).map_err(|err| format!("expected valid JSON: {err}"))?
        }
        _ => match field.enum_values.iter().find(|value| match value {
            JsonValue::String(candidate) => candidate.trim().eq_ignore_ascii_case(raw),
            _ => *value == &json!(raw),
        }) {
            Some(value) => value.clone(),
            None if !field.enum_values.is_empty() => {
                let allowed = field
                    .enum_values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!("expected one of {allowed}"));
            }
            None => JsonValue::String(raw.to_string()),
        },
    };

    validate_custom_preflight_bounds(field, &value)?;
    Ok(value)
}

pub(crate) fn validate_custom_preflight_bounds(
    field: &OperatorFieldSpec,
    value: &JsonValue,
) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = field.minimum {
        if number < minimum {
            return Err(format!("must be >= {minimum}"));
        }
    }
    if let Some(maximum) = field.maximum {
        if number > maximum {
            return Err(format!("must be <= {maximum}"));
        }
    }
    Ok(())
}

pub(crate) fn preflight_answer_labels(answer: &JsonValue, multi_select: bool) -> Vec<String> {
    match answer {
        JsonValue::String(value) if multi_select => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        JsonValue::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Vec::new()
            } else {
                vec![value.to_string()]
            }
        }
        JsonValue::Array(values) => values
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn resources_object_schema(
    resources: &BTreeMap<String, OperatorResourceSpec>,
) -> JsonValue {
    let mut properties = JsonMap::new();
    for (name, resource) in resources.iter().filter(|(_, resource)| resource.exposed) {
        let mut schema = JsonMap::new();
        match name.as_str() {
            "cpu" | "gpu" => {
                schema.insert("type".to_string(), JsonValue::String("integer".to_string()));
                schema.insert("minimum".to_string(), json!(0));
            }
            "memory" | "disk" | "walltime" => {
                schema.insert("type".to_string(), JsonValue::String("string".to_string()));
            }
            _ => {
                schema.insert(
                    "description".to_string(),
                    JsonValue::String("Resource override.".to_string()),
                );
            }
        }
        if let Some(default) = &resource.default {
            schema.insert("default".to_string(), default.clone());
        }
        properties.insert(name.clone(), JsonValue::Object(schema));
    }
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    })
}

pub(crate) fn operator_invocation_preflight_metadata(
    invocation: &OperatorInvocation,
) -> Option<JsonValue> {
    invocation
        .metadata
        .get(OPERATOR_PREFLIGHT_METADATA_KEY)
        .cloned()
}

pub(crate) fn operator_invocation_preflight_param_sources(
    invocation: &OperatorInvocation,
) -> BTreeMap<String, String> {
    operator_invocation_preflight_answered_params(invocation)
        .into_iter()
        .map(|param| (param, OPERATOR_PARAM_SOURCE_USER_PREFLIGHT.to_string()))
        .collect()
}

pub(crate) fn operator_invocation_preflight_answered_params(
    invocation: &OperatorInvocation,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(preflight) = operator_invocation_preflight_metadata(invocation) else {
        return out;
    };
    if let Some(params_by_source) = preflight
        .get("paramsBySource")
        .and_then(JsonValue::as_object)
    {
        out.extend(params_by_source.keys().cloned());
    }
    if let Some(answered) = preflight
        .get("answeredParams")
        .and_then(JsonValue::as_array)
    {
        out.extend(
            answered
                .iter()
                .filter_map(|entry| entry.get("param").and_then(JsonValue::as_str))
                .map(str::to_string),
        );
    }
    out
}

pub(crate) fn operator_param_sources(
    spec: &OperatorSpec,
    supplied_param_names: &BTreeSet<String>,
    preflight_param_names: &BTreeSet<String>,
    effective_params: &BTreeMap<String, JsonValue>,
) -> BTreeMap<String, String> {
    effective_params
        .keys()
        .map(|param| {
            let source = if preflight_param_names.contains(param) {
                OPERATOR_PARAM_SOURCE_USER_PREFLIGHT
            } else if supplied_param_names.contains(param) {
                OPERATOR_PARAM_SOURCE_CALLER
            } else if spec
                .interface
                .params
                .get(param)
                .and_then(|field| field.default.as_ref())
                .is_some()
            {
                OPERATOR_PARAM_SOURCE_DEFAULT
            } else {
                OPERATOR_PARAM_SOURCE_SYSTEM
            };
            (param.clone(), source.to_string())
        })
        .collect()
}

pub(crate) fn apply_param_defaults(
    spec: &OperatorSpec,
    mut params: BTreeMap<String, JsonValue>,
) -> BTreeMap<String, JsonValue> {
    for (name, field) in &spec.interface.params {
        if !params.contains_key(name) {
            if let Some(default) = &field.default {
                params.insert(name.clone(), default.clone());
            }
        }
    }
    params
}

pub(crate) fn reject_unknown_fields<'a>(
    scope: &str,
    names: impl Iterator<Item = &'a String>,
    declared: &BTreeMap<String, OperatorFieldSpec>,
) -> Result<(), OperatorToolError> {
    for name in names {
        if !declared.contains_key(name) {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Unknown {scope} field `{name}`."),
            )
            .with_field(format!("{scope}.{name}"))
            .with_suggested_action("Retry with only fields declared in the operator schema."));
        }
    }
    Ok(())
}

pub(crate) fn validate_field_values(
    scope: &str,
    fields: &BTreeMap<String, OperatorFieldSpec>,
    values: &BTreeMap<String, JsonValue>,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    for (name, field) in fields {
        match values.get(name) {
            Some(value) => validate_field_value(scope, name, field, value)?,
            None if field.required => {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("Required {scope} field `{name}` is missing."),
                )
                .with_field(format!("{scope}.{name}")))
            }
            None => {}
        }
    }
    Ok(())
}

pub(crate) fn field_validation_error_kind(scope: &str) -> &'static str {
    if scope == "structuredOutputs" {
        "output_validation_failed"
    } else {
        "input_validation_failed"
    }
}

pub(crate) fn validate_field_value(
    scope: &str,
    name: &str,
    field: &OperatorFieldSpec,
    value: &JsonValue,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    let field_path = format!("{scope}.{name}");
    if value.is_null() {
        if field.required {
            return Err(OperatorToolError::new(
                error_kind,
                false,
                format!("Required {scope} field `{name}` must not be null."),
            )
            .with_field(field_path));
        }
        return Ok(());
    }

    match field.kind {
        OperatorFieldKind::String | OperatorFieldKind::File | OperatorFieldKind::Directory => {
            let text = value.as_str().ok_or_else(|| {
                OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a string."),
                )
                .with_field(field_path.clone())
            })?;
            if field.non_empty.unwrap_or(false) && text.trim().is_empty() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must not be empty."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::Integer => {
            let number = value.as_i64().or_else(|| value.as_u64().map(|n| n as i64));
            let Some(number) = number else {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be an integer."),
                )
                .with_field(field_path));
            };
            validate_numeric_bounds(scope, name, &field_path, number as f64, field)?;
        }
        OperatorFieldKind::Number => {
            let Some(number) = value.as_f64() else {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a number."),
                )
                .with_field(field_path));
            };
            validate_numeric_bounds(scope, name, &field_path, number, field)?;
        }
        OperatorFieldKind::Boolean => {
            if !value.is_boolean() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a boolean."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::Json => {
            if !value.is_object() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be a JSON object."),
                )
                .with_field(field_path));
            }
        }
        OperatorFieldKind::FileArray | OperatorFieldKind::DirectoryArray => {
            let array = value.as_array().ok_or_else(|| {
                OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must be an array of strings."),
                )
                .with_field(field_path.clone())
            })?;
            if field.non_empty.unwrap_or(false) && array.is_empty() {
                return Err(OperatorToolError::new(
                    error_kind,
                    false,
                    format!("{scope} field `{name}` must not be empty."),
                )
                .with_field(field_path.clone()));
            }
            if let Some(min_size) = field.min_size {
                if (array.len() as u64) < min_size {
                    return Err(OperatorToolError::new(
                        error_kind,
                        false,
                        format!("{scope} field `{name}` requires at least {min_size} item(s)."),
                    )
                    .with_field(field_path.clone()));
                }
            }
            for (index, item) in array.iter().enumerate() {
                if !item.is_string() {
                    return Err(OperatorToolError::new(
                        error_kind,
                        false,
                        format!("{scope} field `{name}[{index}]` must be a string."),
                    )
                    .with_field(format!("{field_path}[{index}]")));
                }
            }
        }
        OperatorFieldKind::Enum => {}
    }

    if !field.enum_values.is_empty() && !field.enum_values.iter().any(|item| item == value) {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!("{scope} field `{name}` is not one of the allowed enum values."),
        )
        .with_field(field_path));
    }
    Ok(())
}

pub(crate) fn validate_numeric_bounds(
    scope: &str,
    name: &str,
    field_path: &str,
    number: f64,
    field: &OperatorFieldSpec,
) -> Result<(), OperatorToolError> {
    let error_kind = field_validation_error_kind(scope);
    if field
        .minimum
        .map(|minimum| number < minimum)
        .unwrap_or(false)
    {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!(
                "{scope} field `{name}` must be >= {}.",
                field.minimum.unwrap_or_default()
            ),
        )
        .with_field(field_path.to_string()));
    }
    if field
        .maximum
        .map(|maximum| number > maximum)
        .unwrap_or(false)
    {
        return Err(OperatorToolError::new(
            error_kind,
            false,
            format!(
                "{scope} field `{name}` must be <= {}.",
                field.maximum.unwrap_or_default()
            ),
        )
        .with_field(field_path.to_string()));
    }
    Ok(())
}

pub(crate) fn apply_resource_defaults_and_overrides(
    spec: &OperatorSpec,
    overrides: BTreeMap<String, JsonValue>,
) -> Result<BTreeMap<String, JsonValue>, OperatorToolError> {
    let mut out = BTreeMap::new();
    for (name, resource) in &spec.resources {
        if let Some(default) = &resource.default {
            out.insert(name.clone(), default.clone());
        }
    }
    for (name, value) in overrides {
        let resource = spec.resources.get(&name).ok_or_else(|| {
            OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` is not declared by this operator."),
            )
            .with_field(format!("resources.{name}"))
        })?;
        if !resource.exposed {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` is not exposed for Agent override."),
            )
            .with_field(format!("resources.{name}")));
        }
        validate_resource_value(&name, &value, resource)?;
        out.insert(name, value);
    }
    for (name, value) in &out {
        if let Some(resource) = spec.resources.get(name) {
            validate_resource_value(name, value, resource)?;
        }
    }
    Ok(out)
}

pub(crate) fn validate_resource_value(
    name: &str,
    value: &JsonValue,
    spec: &OperatorResourceSpec,
) -> Result<(), OperatorToolError> {
    if let (Some(value), Some(minimum)) = (
        value.as_f64(),
        spec.min.as_ref().and_then(JsonValue::as_f64),
    ) {
        if value < minimum {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` must be >= {minimum}."),
            )
            .with_field(format!("resources.{name}")));
        }
    }
    if let (Some(value), Some(maximum)) = (
        value.as_f64(),
        spec.max.as_ref().and_then(JsonValue::as_f64),
    ) {
        if value > maximum {
            return Err(OperatorToolError::new(
                "invalid_arguments",
                false,
                format!("Resource `{name}` must be <= {maximum}."),
            )
            .with_field(format!("resources.{name}")));
        }
    }
    Ok(())
}

pub(crate) fn apply_equal_bindings(
    spec: &OperatorSpec,
    params: &mut BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
) -> Result<(), OperatorToolError> {
    for binding in &spec.bindings {
        if binding.mode != "equal" {
            continue;
        }
        let param = params.get(&binding.param).cloned();
        let resource = resources.get(&binding.resource).cloned();
        match (param, resource) {
            (Some(param), Some(resource)) if param != resource => {
                return Err(OperatorToolError::new(
                    "invalid_arguments",
                    false,
                    format!(
                        "Binding requires params.{} == resources.{}.",
                        binding.param, binding.resource
                    ),
                )
                .with_field(format!("params.{}", binding.param)));
            }
            (None, Some(resource)) => {
                params.insert(binding.param.clone(), resource);
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn canonicalize_inputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    inputs: BTreeMap<String, JsonValue>,
    is_ssh: bool,
) -> Result<BTreeMap<String, JsonValue>, OperatorToolError> {
    let mut out = BTreeMap::new();
    for (name, field) in &spec.interface.inputs {
        let value = inputs.get(name).cloned().or_else(|| field.default.clone());
        let Some(value) = value else {
            if field.required {
                return Err(OperatorToolError::new(
                    "input_validation_failed",
                    false,
                    format!("Required input `{name}` is missing."),
                )
                .with_field(format!("inputs.{name}")));
            }
            continue;
        };
        if !field.required
            && field.kind.is_path_like()
            && value.as_str().map(str::trim).is_some_and(str::is_empty)
        {
            continue;
        }
        validate_field_value("inputs", name, field, &value)?;
        let canonical = if field.kind.is_path_like() {
            canonicalize_path_value(ctx, &field.kind, name, value, is_ssh)?
        } else {
            value
        };
        out.insert(name.clone(), canonical);
    }
    Ok(out)
}

pub(crate) fn canonicalize_path_value(
    ctx: &crate::domain::tools::ToolContext,
    kind: &OperatorFieldKind,
    name: &str,
    value: JsonValue,
    is_ssh: bool,
) -> Result<JsonValue, OperatorToolError> {
    if kind.is_array() {
        let array = value.as_array().ok_or_else(|| {
            OperatorToolError::new(
                "input_validation_failed",
                false,
                format!("Input `{name}` must be an array of paths."),
            )
            .with_field(format!("inputs.{name}"))
        })?;
        let values = array
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let path = item.as_str().ok_or_else(|| {
                    OperatorToolError::new(
                        "input_validation_failed",
                        false,
                        format!("Input `{name}[{idx}]` must be a path string."),
                    )
                    .with_field(format!("inputs.{name}[{idx}]"))
                })?;
                canonicalize_one_path(ctx, path, is_ssh).map(JsonValue::String)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(JsonValue::Array(values))
    } else {
        let path = value.as_str().ok_or_else(|| {
            OperatorToolError::new(
                "input_validation_failed",
                false,
                format!("Input `{name}` must be a path string."),
            )
            .with_field(format!("inputs.{name}"))
        })?;
        canonicalize_one_path(ctx, path, is_ssh).map(JsonValue::String)
    }
}

pub(crate) fn canonicalize_one_path(
    ctx: &crate::domain::tools::ToolContext,
    raw: &str,
    is_ssh: bool,
) -> Result<String, OperatorToolError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(OperatorToolError::new(
            "input_validation_failed",
            false,
            "Input path must not be empty.",
        ));
    }
    if is_ssh {
        return Ok(crate::domain::tools::env_store::remote_path(ctx, trimmed));
    }
    let path = PathBuf::from(trimmed);
    let full = if path.is_absolute() {
        path
    } else {
        ctx.project_root.join(path)
    };
    let canonical = full.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "input_validation_failed",
            false,
            format!("Input path `{trimmed}` is not accessible: {err}"),
        )
    })?;
    let project = ctx
        .project_root
        .canonicalize()
        .unwrap_or_else(|_| ctx.project_root.clone());
    if !canonical.starts_with(&project) {
        return Err(OperatorToolError::new(
            "input_validation_failed",
            false,
            format!(
                "Input path `{}` is outside the project root.",
                canonical.display()
            ),
        )
        .with_suggested_action("Move or reference files under the current project root."));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

pub(crate) fn expand_argv(
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
    params: &BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
    run_dir: &str,
) -> Result<Vec<String>, OperatorToolError> {
    let mut argv = Vec::new();
    for (index, token) in spec.execution.argv.iter().enumerate() {
        if let Some(expanded) = expand_exact_array_token(token, inputs) {
            argv.extend(expanded);
            continue;
        }
        let mut replaced = replace_token_vars(token, spec, inputs, params, resources, run_dir)?;
        if replaced.contains('/') && !Path::new(&replaced).is_absolute() {
            let plugin_file = spec.source.plugin_root.join(&replaced);
            if plugin_file.is_file() || index == 0 {
                replaced = plugin_file.to_string_lossy().into_owned();
            }
        }
        argv.push(replaced);
    }
    Ok(argv)
}

pub(crate) fn expand_exact_array_token(
    token: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> Option<Vec<String>> {
    let key = exact_var_key(token)?;
    let name = key.strip_prefix("inputs.")?;
    inputs.get(name)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    })
}

pub(crate) fn exact_var_key(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        return Some(trimmed[2..trimmed.len() - 1].trim().to_string());
    }
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        return Some(trimmed[2..trimmed.len() - 2].trim().to_string());
    }
    None
}

pub(crate) fn replace_token_vars(
    token: &str,
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
    params: &BTreeMap<String, JsonValue>,
    resources: &BTreeMap<String, JsonValue>,
    run_dir: &str,
) -> Result<String, OperatorToolError> {
    let mut out = token.to_string();
    let outdir = format!("{run_dir}/out.tmp");
    let workdir = format!("{run_dir}/work");
    let replacements = [
        ("workdir".to_string(), workdir),
        ("outdir".to_string(), outdir),
        (
            "plugin_dir".to_string(),
            spec.source.plugin_root.to_string_lossy().into_owned(),
        ),
    ];
    for (key, value) in replacements {
        out = out.replace(&format!("${{{key}}}"), &value);
        out = out.replace(&format!("{{{{ {key} }}}}"), &value);
    }
    for (prefix, map) in [
        ("inputs", inputs),
        ("params", params),
        ("resources", resources),
    ] {
        for (name, value) in map {
            let rendered = value_to_arg_string(value);
            out = out.replace(&format!("${{{prefix}.{name}}}"), &rendered);
            out = out.replace(&format!("{{{{ {prefix}.{name} }}}}"), &rendered);
        }
    }
    clear_missing_optional_vars(
        &mut out,
        "inputs",
        &spec.interface.inputs,
        inputs,
        |field| !field.required,
    );
    clear_missing_optional_vars(
        &mut out,
        "params",
        &spec.interface.params,
        params,
        |field| !field.required,
    );
    clear_missing_optional_vars(&mut out, "resources", &spec.resources, resources, |field| {
        !field.exposed
    });
    Ok(out)
}

pub(crate) fn clear_missing_optional_vars<T>(
    out: &mut String,
    prefix: &str,
    specs: &BTreeMap<String, T>,
    provided: &BTreeMap<String, JsonValue>,
    optional: impl Fn(&T) -> bool,
) {
    for (name, spec) in specs {
        if optional(spec) && !provided.contains_key(name) {
            *out = out.replace(&format!("${{{prefix}.{name}}}"), "");
            *out = out.replace(&format!("{{{{ {prefix}.{name} }}}}"), "");
        }
    }
}

pub(crate) fn value_to_arg_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Array(items) => items
            .iter()
            .map(value_to_arg_string)
            .collect::<Vec<_>>()
            .join(" "),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}
