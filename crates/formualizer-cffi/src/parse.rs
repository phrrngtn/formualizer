use formualizer_common::LiteralValue;
use formualizer_parse::parser::{ASTNode as CoreASTNode, ASTNodeType, ReferenceType};
use formualizer_parse::tokenizer::Token as CoreToken;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CffiASTNode {
    Number {
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Text {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Boolean {
        value: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Empty {
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Error {
        kind: String,
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Reference {
        sheet: Option<String>,
        reference: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Function {
        name: String,
        args: Vec<CffiASTNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    BinaryOp {
        op: String,
        left: Box<CffiASTNode>,
        right: Box<CffiASTNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    UnaryOp {
        op: String,
        operand: Box<CffiASTNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Array {
        elements: Vec<Vec<CffiASTNode>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    Call {
        callee: Box<CffiASTNode>,
        args: Vec<CffiASTNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
}

impl CffiASTNode {
    /// Collect every function name called anywhere in the tree (with duplicates,
    /// in traversal order). Callers dedupe as they wish.
    pub fn collect_function_names(&self, out: &mut Vec<String>) {
        match self {
            CffiASTNode::Function { name, args, .. } => {
                out.push(name.clone());
                for a in args {
                    a.collect_function_names(out);
                }
            }
            CffiASTNode::Call { callee, args, .. } => {
                callee.collect_function_names(out);
                for a in args {
                    a.collect_function_names(out);
                }
            }
            CffiASTNode::BinaryOp { left, right, .. } => {
                left.collect_function_names(out);
                right.collect_function_names(out);
            }
            CffiASTNode::UnaryOp { operand, .. } => operand.collect_function_names(out),
            CffiASTNode::Array { elements, .. } => {
                for row in elements {
                    for e in row {
                        e.collect_function_names(out);
                    }
                }
            }
            _ => {}
        }
    }
}

// ── R1C1 canonical rendering (translation-invariant fingerprint form) ──
// Ported from formulon's daf.rs. Relative references render as R[dr]C[dc]
// offsets from the anchor cell, so two drag-filled cells (which differ only by a
// uniform translation of their relative refs) produce the *identical* string,
// while a structurally different formula does not. Fully parenthesised so
// structure is unambiguous.
fn r1c1_axis(prefix: char, coord: u32, abs: bool, anchor: u32) -> String {
    if abs {
        format!("{prefix}{coord}")
    } else {
        format!("{prefix}[{}]", coord as i64 - anchor as i64)
    }
}

fn r1c1_ref(rt: &ReferenceType, row: u32, col: u32) -> String {
    let sheet = |s: &Option<String>| s.as_deref().map(|x| format!("{x}!")).unwrap_or_default();
    match rt {
        ReferenceType::Cell { sheet: sh, row: r, col: c, row_abs, col_abs } => format!(
            "{}{}{}",
            sheet(sh),
            r1c1_axis('R', *r, *row_abs, row),
            r1c1_axis('C', *c, *col_abs, col),
        ),
        ReferenceType::Range {
            sheet: sh, start_row, start_col, end_row, end_col,
            start_row_abs, start_col_abs, end_row_abs, end_col_abs,
        } => {
            let end = |ro: &Option<u32>, ra: bool, co: &Option<u32>, ca: bool| format!(
                "{}{}",
                ro.map(|v| r1c1_axis('R', v, ra, row)).unwrap_or_default(),
                co.map(|v| r1c1_axis('C', v, ca, col)).unwrap_or_default(),
            );
            format!(
                "{}{}:{}",
                sheet(sh),
                end(start_row, *start_row_abs, start_col, *start_col_abs),
                end(end_row, *end_row_abs, end_col, *end_col_abs),
            )
        }
        other => format!("{other}"), // 3D / external / table / named: A1 Display fallback
    }
}

/// Canonical R1C1 serialization of a formula AST at cell (`row`, `col`).
pub fn render_r1c1(node: &CoreASTNode, row: u32, col: u32) -> String {
    let kids = |xs: &[CoreASTNode]| {
        xs.iter().map(|a| render_r1c1(a, row, col)).collect::<Vec<_>>().join(",")
    };
    match &node.node_type {
        ASTNodeType::Literal(v) => format!("{v}"),
        ASTNodeType::Reference { reference, .. } => r1c1_ref(reference, row, col),
        ASTNodeType::UnaryOp { op, expr } => format!("({op} {})", render_r1c1(expr, row, col)),
        ASTNodeType::BinaryOp { op, left, right } => {
            format!("({} {op} {})", render_r1c1(left, row, col), render_r1c1(right, row, col))
        }
        ASTNodeType::Function { name, args } => format!("{name}({})", kids(args)),
        ASTNodeType::Call { callee, args } => {
            format!("{}({})", render_r1c1(callee, row, col), kids(args))
        }
        ASTNodeType::Array(rows) => {
            format!("{{{}}}", rows.iter().map(|r| kids(r)).collect::<Vec<_>>().join(";"))
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CffiToken {
    pub value: String,
    pub token_type: String,
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<[usize; 2]>,
}

impl CffiASTNode {
    pub fn from_core(node: &CoreASTNode, include_spans: bool) -> Self {
        let span = if include_spans {
            node.source_token.as_ref().map(|t| [t.start, t.end])
        } else {
            None
        };

        match &node.node_type {
            ASTNodeType::Literal(lit) => match lit {
                LiteralValue::Int(i) => CffiASTNode::Number {
                    value: *i as f64,
                    span,
                },
                LiteralValue::Number(n) => CffiASTNode::Number { value: *n, span },
                LiteralValue::Text(s) => CffiASTNode::Text {
                    value: s.clone(),
                    span,
                },
                LiteralValue::Boolean(b) => CffiASTNode::Boolean { value: *b, span },
                LiteralValue::Empty => CffiASTNode::Empty { span },
                LiteralValue::Error(e) => CffiASTNode::Error {
                    kind: format!("{:?}", e.kind),
                    message: e.message.clone(),
                    span,
                },
                LiteralValue::Array(arr) => CffiASTNode::Array {
                    elements: arr
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|v| Self::from_literal(v, include_spans))
                                .collect()
                        })
                        .collect(),
                    span,
                },
                _ => CffiASTNode::Text {
                    value: lit.to_string(),
                    span,
                },
            },
            ASTNodeType::Reference {
                original,
                reference,
            } => {
                let sheet = match reference {
                    ReferenceType::Cell { sheet, .. } => sheet.clone(),
                    ReferenceType::Range { sheet, .. } => sheet.clone(),
                    _ => None,
                };
                CffiASTNode::Reference {
                    sheet,
                    reference: original.clone(),
                    span,
                }
            }
            ASTNodeType::Function { name, args } => CffiASTNode::Function {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| Self::from_core(a, include_spans))
                    .collect(),
                span,
            },
            ASTNodeType::BinaryOp { op, left, right } => CffiASTNode::BinaryOp {
                op: op.clone(),
                left: Box::new(Self::from_core(left, include_spans)),
                right: Box::new(Self::from_core(right, include_spans)),
                span,
            },
            ASTNodeType::UnaryOp { op, expr } => CffiASTNode::UnaryOp {
                op: op.clone(),
                operand: Box::new(Self::from_core(expr, include_spans)),
                span,
            },
            ASTNodeType::Array(rows) => CffiASTNode::Array {
                elements: rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|a| Self::from_core(a, include_spans))
                            .collect()
                    })
                    .collect(),
                span,
            },
            ASTNodeType::Call { callee, args } => CffiASTNode::Call {
                callee: Box::new(Self::from_core(callee, include_spans)),
                args: args
                    .iter()
                    .map(|a| Self::from_core(a, include_spans))
                    .collect(),
                span,
            },
        }
    }

    fn from_literal(lit: &LiteralValue, _include_spans: bool) -> Self {
        match lit {
            LiteralValue::Int(i) => CffiASTNode::Number {
                value: *i as f64,
                span: None,
            },
            LiteralValue::Number(n) => CffiASTNode::Number {
                value: *n,
                span: None,
            },
            LiteralValue::Text(s) => CffiASTNode::Text {
                value: s.clone(),
                span: None,
            },
            LiteralValue::Boolean(b) => CffiASTNode::Boolean {
                value: *b,
                span: None,
            },
            LiteralValue::Empty => CffiASTNode::Empty { span: None },
            LiteralValue::Error(e) => CffiASTNode::Error {
                kind: format!("{:?}", e.kind),
                message: e.message.clone(),
                span: None,
            },
            LiteralValue::Array(arr) => CffiASTNode::Array {
                elements: arr
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|v| Self::from_literal(v, _include_spans))
                            .collect()
                    })
                    .collect(),
                span: None,
            },
            _ => CffiASTNode::Text {
                value: lit.to_string(),
                span: None,
            },
        }
    }
}

impl CffiToken {
    pub fn from_core(token: &CoreToken, include_spans: bool) -> Self {
        CffiToken {
            value: token.value.clone(),
            token_type: format!("{:?}", token.token_type),
            subtype: format!("{:?}", token.subtype),
            span: if include_spans {
                Some([token.start, token.end])
            } else {
                None
            },
        }
    }
}
