use std::collections::HashMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::engines::ast::{locate_callsites, ScanConfig};
use crate::engines::autopatch::{
    execution_order, parse_workflow, plan_maintenance, render_markdown, synthesize_contract_tests,
    validate_workflow, MaintenancePlan, VerificationSpec,
};
use crate::engines::schema::{diff_specs, parse_spec};

#[pyfunction]
fn sandbox_apply(worktree_path: &str, block_network: bool) -> PyResult<bool> {
    match crate::sandbox::apply(worktree_path, block_network) {
        Ok(()) => Ok(true),
        Err(e) => {
            if !crate::sandbox::check_supported() {
                Ok(false)
            } else {
                Err(PyValueError::new_err(format!(
                    "Sandbox application failed: {e}"
                )))
            }
        }
    }
}

#[pyfunction]
fn sandbox_check_supported() -> PyResult<HashMap<String, String>> {
    let info = crate::sandbox::get_info();
    let mut result = HashMap::new();
    result.insert("supported".to_string(), info.supported.to_string());
    result.insert("platform".to_string(), info.platform);
    result.insert("details".to_string(), info.details);
    Ok(result)
}

#[pyfunction]
fn route_and_compress(content: &str) -> String {
    crate::engines::compression::route_and_compress(content)
}

#[pyfunction]
fn schema_diff(old_json: &str, new_json: &str) -> PyResult<String> {
    let old = parse_spec(old_json).map_err(PyValueError::new_err)?;
    let new = parse_spec(new_json).map_err(PyValueError::new_err)?;
    let diff = diff_specs(&old, &new);
    serde_json::to_string_pretty(&diff).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn ast_locate_callsites(root_dir: &str, config_json: &str) -> PyResult<String> {
    let config: ScanConfig = if config_json.is_empty() || config_json == "{}" {
        ScanConfig::default()
    } else {
        serde_json::from_str(config_json).map_err(|e| PyValueError::new_err(e.to_string()))?
    };
    let result = locate_callsites(root_dir, &config);
    serde_json::to_string_pretty(&result).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn autopatch_plan(
    old_json: &str,
    new_json: &str,
    repo_root: &str,
    config_json: &str,
) -> PyResult<String> {
    let config: ScanConfig = if config_json.is_empty() || config_json == "{}" {
        ScanConfig::default()
    } else {
        serde_json::from_str(config_json).map_err(|e| PyValueError::new_err(e.to_string()))?
    };
    let plan =
        plan_maintenance(old_json, new_json, repo_root, &config).map_err(PyValueError::new_err)?;
    serde_json::to_string_pretty(&plan).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn synthesize_contracts(
    api_name: &str,
    old_ver: &str,
    new_ver: &str,
    specs_json: &str,
    lang: &str,
) -> PyResult<String> {
    let specs: Vec<VerificationSpec> =
        serde_json::from_str(specs_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let suite = synthesize_contract_tests(api_name, old_ver, new_ver, &specs);
    match lang.to_lowercase().as_str() {
        "ts" | "typescript" | "js" | "javascript" => Ok(suite.render_typescript()),
        "py" | "python" => Ok(suite.render_python()),
        _ => Err(PyValueError::new_err(format!(
            "Unsupported contract language: '{lang}' (supported: 'typescript', 'python')"
        ))),
    }
}

#[pyfunction]
fn render_report_markdown(plan_json: &str) -> PyResult<String> {
    let plan: MaintenancePlan =
        serde_json::from_str(plan_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(render_markdown(&plan))
}

#[pyfunction]
fn workflow_validate(workflow_json: &str) -> PyResult<Vec<String>> {
    let wf = parse_workflow(workflow_json).map_err(PyValueError::new_err)?;
    match validate_workflow(&wf) {
        Ok(()) => Ok(Vec::new()),
        Err(errs) => Ok(errs),
    }
}

#[pyfunction]
fn workflow_execution_order(workflow_json: &str) -> PyResult<Vec<String>> {
    let wf = parse_workflow(workflow_json).map_err(PyValueError::new_err)?;
    execution_order(&wf).map_err(PyValueError::new_err)
}

#[pyfunction]
fn inventory_scan(repo_root: &str) -> PyResult<String> {
    let inv = crate::engines::autopatch::run_inventory(repo_root);
    serde_json::to_string_pretty(&inv).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn trust_report_render(plan_json: &str) -> PyResult<String> {
    let plan: MaintenancePlan =
        serde_json::from_str(plan_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(crate::engines::autopatch::render_trust_report_cli(&plan))
}

#[pyfunction]
fn patch_apply(repo_root: &str, plan_json: &str, dry_run: bool) -> PyResult<String> {
    let plan: MaintenancePlan =
        serde_json::from_str(plan_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let results = crate::engines::autopatch::patch_plan_targets(repo_root, &plan, dry_run)
        .map_err(PyValueError::new_err)?;
    serde_json::to_string_pretty(&results).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (repo_root="."))]
fn dependency_graph_build(repo_root: &str) -> PyResult<String> {
    let graph = crate::engines::graph::build_external_dependency_graph(repo_root, None);
    serde_json::to_string_pretty(&graph).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (repo_root="."))]
fn dependency_graph_audit(repo_root: &str) -> PyResult<String> {
    let graph = crate::engines::graph::build_external_dependency_graph(repo_root, None);
    let summary = graph.audit_summary();
    serde_json::to_string_pretty(&summary).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sandbox_apply, m)?)?;
    m.add_function(wrap_pyfunction!(sandbox_check_supported, m)?)?;
    m.add_function(wrap_pyfunction!(route_and_compress, m)?)?;
    m.add_function(wrap_pyfunction!(schema_diff, m)?)?;
    m.add_function(wrap_pyfunction!(ast_locate_callsites, m)?)?;
    m.add_function(wrap_pyfunction!(autopatch_plan, m)?)?;
    m.add_function(wrap_pyfunction!(synthesize_contracts, m)?)?;
    m.add_function(wrap_pyfunction!(render_report_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(workflow_validate, m)?)?;
    m.add_function(wrap_pyfunction!(workflow_execution_order, m)?)?;
    m.add_function(wrap_pyfunction!(inventory_scan, m)?)?;
    m.add_function(wrap_pyfunction!(trust_report_render, m)?)?;
    m.add_function(wrap_pyfunction!(patch_apply, m)?)?;
    m.add_function(wrap_pyfunction!(dependency_graph_build, m)?)?;
    m.add_function(wrap_pyfunction!(dependency_graph_audit, m)?)?;
    Ok(())
}
