use crate::ast::Span;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum Phase {
    Lex,
    Parse,
    Resolve,
    Type,
    Lower,
    Codegen,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuggestedFix {
    pub strategy: String,
    pub patch: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub phase: Phase,
    pub node_id: Option<String>,
    pub span: Span,
    pub message: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub suggested_fix: SuggestedFix,
}

impl Diagnostic {
    pub fn error(
        code: &str,
        phase: Phase,
        node_id: Option<String>,
        span: Span,
        message: impl Into<String>,
        expected: Option<String>,
        actual: Option<String>,
        fix: SuggestedFix,
    ) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Error,
            phase,
            node_id,
            span,
            message: message.into(),
            expected,
            actual,
            suggested_fix: fix,
        }
    }

    pub fn warning(
        code: &str,
        phase: Phase,
        node_id: Option<String>,
        span: Span,
        message: impl Into<String>,
        expected: Option<String>,
        actual: Option<String>,
        fix: SuggestedFix,
    ) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Warning,
            phase,
            node_id,
            span,
            message: message.into(),
            expected,
            actual,
            suggested_fix: fix,
        }
    }

    pub fn has_errors(diags: &[Diagnostic]) -> bool {
        diags.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("diagnostics are JSON serializable")
    }
}

pub fn fix(strategy: &str, patch: impl Into<String>) -> SuggestedFix {
    SuggestedFix {
        strategy: strategy.to_string(),
        patch: patch.into(),
    }
}

pub fn emit_json_lines(diags: &[Diagnostic]) {
    for diag in diags {
        println!("{}", diag.to_json());
    }
}

pub fn emit_json_batch(diags: &[Diagnostic]) {
    println!(
        "{}",
        serde_json::to_string(diags).expect("diagnostics are JSON serializable")
    );
}


