// TODO(R004): stub — will be replaced by crate-level diagnostics module

use crate::error::DiagnosticReport;

#[derive(Debug)]
pub enum DiagnosticWarning {
    // TODO(R004): stub variants removed
}

impl DiagnosticWarning {
    pub fn name(&self) -> &'static str {
        match *self {}
    }
}

impl From<&DiagnosticWarning> for DiagnosticReport {
    fn from(diag: &DiagnosticWarning) -> Self {
        match *diag {}
    }
}

#[derive(Debug)]
pub enum DiagnosticError {
    // TODO(R004): stub variants removed
}

impl DiagnosticError {
    pub fn name(&self) -> &'static str {
        match *self {}
    }
}

impl From<&DiagnosticError> for DiagnosticReport {
    fn from(diag: &DiagnosticError) -> Self {
        match *diag {}
    }
}
