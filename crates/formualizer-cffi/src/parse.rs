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

// ── Structured references (fz_parse_references) ──────────────────────────────
// Ported and extended from formulon's wasm `references` op. It folds the
// dynamic-array reference operators the parser flattens into generic unary ops —
// spill `#` (postfix) and implicit-intersection `@` (prefix) — back into single
// classified entries. The extension over the wasm version: emit resolved 1-based
// numeric coordinates (A1 = row 1, col 1) so the dependency graph can join by box
// overlap / anchor identity without re-parsing text. Unresolvable refs
// (named/table/external) carry a NULL box for a SQL name pre-pass to fill in.
//
// AST-based, so it only sees what the parser accepts: trimmed refs (`A1.:.J100`)
// are rejected upstream and surface as a parse error, not a reference.

/// One classified reference. `Option` numeric fields serialize as absent (JSON) /
/// null; the packed encoder carries an explicit presence bitmask instead.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct CffiRef {
    /// cell | range | cell3d | range3d | external | table | named | spill | implicit_intersection
    pub kind: String,
    /// The reference exactly as written ("A1", "A1#", "@B1:B5", "Sheet1!C2").
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// For 3D refs (`Sheet1:Sheet3!…`), the last sheet in the span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r1: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c1: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r2: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c2: Option<u32>,
    /// True only when the axis is fully absolute (`$` on both bounds) — so it does
    /// not sweep when the enclosing region is drag-filled. Mixed abs → false
    /// (conservative: the axis is treated as sweeping).
    pub row_abs: bool,
    pub col_abs: bool,
    /// Whole-column (open rows) / whole-row (open cols): a bound was `None`.
    pub open_rows: bool,
    pub open_cols: bool,
    /// Spill / `@`: the inner anchor reference as written ("A1" for "A1#"). Its
    /// sheet is the ref's own `sheet` (the box is the inner cell's), so there is
    /// no separate anchor_sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Spill: numeric origin of the producing region — the anchor-identity join key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_row: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_col: Option<u32>,
    /// The reference operator applied, if any ("#" or "@").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
struct RefCoords {
    sheet: Option<String>,
    sheet_end: Option<String>,
    r1: Option<u32>,
    c1: Option<u32>,
    r2: Option<u32>,
    c2: Option<u32>,
    row_abs: bool,
    col_abs: bool,
    open_rows: bool,
    open_cols: bool,
}

fn ref_kind(r: &ReferenceType) -> &'static str {
    match r {
        ReferenceType::Cell { .. } => "cell",
        ReferenceType::Range { .. } => "range",
        ReferenceType::Cell3D { .. } => "cell3d",
        ReferenceType::Range3D { .. } => "range3d",
        ReferenceType::External(_) => "external",
        ReferenceType::Table(_) => "table",
        ReferenceType::NamedRange(_) => "named",
    }
}

/// Resolve a `ReferenceType` into grid coordinates. Named/table/external stay NULL.
fn coords_of(rt: &ReferenceType) -> RefCoords {
    match rt {
        ReferenceType::Cell { sheet, row, col, row_abs, col_abs } => RefCoords {
            sheet: sheet.clone(),
            r1: Some(*row), c1: Some(*col), r2: Some(*row), c2: Some(*col),
            row_abs: *row_abs, col_abs: *col_abs,
            ..Default::default()
        },
        ReferenceType::Range {
            sheet, start_row, start_col, end_row, end_col,
            start_row_abs, start_col_abs, end_row_abs, end_col_abs,
        } => RefCoords {
            sheet: sheet.clone(),
            r1: *start_row, c1: *start_col, r2: *end_row, c2: *end_col,
            row_abs: *start_row_abs && *end_row_abs,
            col_abs: *start_col_abs && *end_col_abs,
            open_rows: start_row.is_none() || end_row.is_none(),
            open_cols: start_col.is_none() || end_col.is_none(),
            ..Default::default()
        },
        ReferenceType::Cell3D { sheet_first, sheet_last, row, col, row_abs, col_abs } => RefCoords {
            sheet: Some(sheet_first.clone()), sheet_end: Some(sheet_last.clone()),
            r1: Some(*row), c1: Some(*col), r2: Some(*row), c2: Some(*col),
            row_abs: *row_abs, col_abs: *col_abs,
            ..Default::default()
        },
        ReferenceType::Range3D {
            sheet_first, sheet_last, start_row, start_col, end_row, end_col,
            start_row_abs, start_col_abs, end_row_abs, end_col_abs,
        } => RefCoords {
            sheet: Some(sheet_first.clone()), sheet_end: Some(sheet_last.clone()),
            r1: *start_row, c1: *start_col, r2: *end_row, c2: *end_col,
            row_abs: *start_row_abs && *end_row_abs,
            col_abs: *start_col_abs && *end_col_abs,
            open_rows: start_row.is_none() || end_row.is_none(),
            open_cols: start_col.is_none() || end_col.is_none(),
            ..Default::default()
        },
        // Unresolvable to a grid box without workbook metadata.
        ReferenceType::External(_) | ReferenceType::Table(_) | ReferenceType::NamedRange(_) => {
            RefCoords::default()
        }
    }
}

/// Min-start / max-end over a node's whole subtree of source tokens.
fn node_span(node: &CoreASTNode) -> Option<(usize, usize)> {
    fn walk(n: &CoreASTNode, lo: &mut usize, hi: &mut usize, any: &mut bool) {
        if let Some(t) = &n.source_token {
            *lo = (*lo).min(t.start);
            *hi = (*hi).max(t.end);
            *any = true;
        }
        match &n.node_type {
            ASTNodeType::UnaryOp { expr, .. } => walk(expr, lo, hi, any),
            ASTNodeType::BinaryOp { left, right, .. } => {
                walk(left, lo, hi, any);
                walk(right, lo, hi, any);
            }
            ASTNodeType::Function { args, .. } => args.iter().for_each(|a| walk(a, lo, hi, any)),
            ASTNodeType::Call { callee, args } => {
                walk(callee, lo, hi, any);
                args.iter().for_each(|a| walk(a, lo, hi, any));
            }
            ASTNodeType::Array(rows) => {
                rows.iter().for_each(|r| r.iter().for_each(|c| walk(c, lo, hi, any)))
            }
            _ => {}
        }
    }
    let (mut lo, mut hi, mut any) = (usize::MAX, 0usize, false);
    walk(node, &mut lo, &mut hi, &mut any);
    any.then_some((lo, hi))
}

/// For a folded `#`/`@` operand, the inner reference's `(sheet, row, col, text)`
/// (numeric only when the anchor is a single cell).
fn anchor_info(expr: &CoreASTNode) -> Option<(Option<String>, Option<u32>, Option<u32>, String)> {
    if let ASTNodeType::Reference { original, reference } = &expr.node_type {
        let (s, r, c) = match reference {
            ReferenceType::Cell { sheet, row, col, .. } => (sheet.clone(), Some(*row), Some(*col)),
            _ => (None, None, None),
        };
        Some((s, r, c, original.clone()))
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    node: &CoreASTNode,
    formula: &str,
    kind: &str,
    rt: &ReferenceType,
    anchor: Option<(Option<String>, Option<u32>, Option<u32>, String)>,
    op: Option<&str>,
    out: &mut Vec<CffiRef>,
) {
    let Some((start, end)) = node_span(node) else { return };
    let c = coords_of(rt);
    // The anchor's sheet equals the ref's own `sheet`, so we discard it here.
    let (_a_sheet, a_row, a_col, a_text) = match anchor {
        Some((s, r, cc, t)) => (s, r, cc, Some(t)),
        None => (None, None, None, None),
    };
    out.push(CffiRef {
        kind: kind.to_string(),
        text: formula.get(start..end).unwrap_or_default().to_string(),
        sheet: c.sheet,
        sheet_end: c.sheet_end,
        r1: c.r1, c1: c.c1, r2: c.r2, c2: c.c2,
        row_abs: c.row_abs, col_abs: c.col_abs,
        open_rows: c.open_rows, open_cols: c.open_cols,
        anchor: a_text, anchor_row: a_row, anchor_col: a_col,
        operator: op.map(str::to_string),
        start, end,
    });
}

fn collect_refs(node: &CoreASTNode, formula: &str, out: &mut Vec<CffiRef>) {
    match &node.node_type {
        // Reference operators: fold the wrapper into one classified entry. The box
        // is the inner reference's; the span/text covers the whole `A1#` / `@A1`.
        ASTNodeType::UnaryOp { op, expr } if op == "#" => {
            if let ASTNodeType::Reference { reference, .. } = &expr.node_type {
                emit(node, formula, "spill", reference, anchor_info(expr), Some("#"), out);
            } else {
                collect_refs(expr, formula, out);
            }
        }
        ASTNodeType::UnaryOp { op, expr } if op == "@" => {
            if let ASTNodeType::Reference { reference, .. } = &expr.node_type {
                emit(node, formula, "implicit_intersection", reference, anchor_info(expr), Some("@"), out);
            } else {
                collect_refs(expr, formula, out);
            }
        }
        // Any other unary (arithmetic `-`, `+`, postfix `%`) does not affect the ref.
        ASTNodeType::UnaryOp { expr, .. } => collect_refs(expr, formula, out),
        ASTNodeType::BinaryOp { left, right, .. } => {
            collect_refs(left, formula, out);
            collect_refs(right, formula, out);
        }
        ASTNodeType::Function { args, .. } => args.iter().for_each(|a| collect_refs(a, formula, out)),
        ASTNodeType::Call { callee, args } => {
            collect_refs(callee, formula, out);
            args.iter().for_each(|a| collect_refs(a, formula, out));
        }
        ASTNodeType::Array(rows) => {
            rows.iter().for_each(|r| r.iter().for_each(|c| collect_refs(c, formula, out)))
        }
        ASTNodeType::Reference { reference, .. } => {
            emit(node, formula, ref_kind(reference), reference, None, None, out)
        }
        ASTNodeType::Literal(_) => {}
    }
}

/// Parse a formula and collect its classified references (source order).
pub fn parse_references(
    formula: &str,
    dialect: formualizer_parse::FormulaDialect,
) -> Result<Vec<CffiRef>, String> {
    let ast = formualizer_parse::parser::parse_with_dialect(formula, dialect)
        .map_err(|e| e.to_string())?;
    let mut refs = Vec::new();
    collect_refs(&ast, formula, &mut refs);
    Ok(refs)
}

// ── Packed binary encoding (fz_parse_references_packed) ──────────────────────
// A fixed-layout, string-arena-free wire for hosts that want typed columns
// without paying JSON (de)serialization — e.g. DuckDB building a LIST(STRUCT).
// Little-endian. Numeric fields are always written; a presence bitmask marks
// which are valid (NULL otherwise). Strings are length-prefixed inline.
//
//   u32 magic = 0x584C5231 ("XLR1")
//   u32 count
//   count × record:
//     u8  kind_tag   0..8 (see kind_tag)
//     u8  flags      bit0 row_abs, bit1 col_abs, bit2 open_rows, bit3 open_cols
//     u8  present    bit0 r1, bit1 c1, bit2 r2, bit3 c2, bit4 anchor_row, bit5 anchor_col
//     u8  _pad
//     u32 r1,c1,r2,c2,anchor_row,anchor_col      (value, or 0 when not present)
//     u32 start, end
//     str text, sheet, sheet_end, anchor, operator   (each: u32 len + bytes; len 0 = NULL)

pub const REFS_PACKED_MAGIC: u32 = 0x584C_5231; // "XLR1"

fn kind_tag(k: &str) -> u8 {
    match k {
        "cell" => 0,
        "range" => 1,
        "cell3d" => 2,
        "range3d" => 3,
        "external" => 4,
        "table" => 5,
        "named" => 6,
        "spill" => 7,
        "implicit_intersection" => 8,
        _ => 255,
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

pub fn pack_refs(refs: &[CffiRef]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + refs.len() * 48);
    out.extend_from_slice(&REFS_PACKED_MAGIC.to_le_bytes());
    out.extend_from_slice(&(refs.len() as u32).to_le_bytes());
    for r in refs {
        out.push(kind_tag(&r.kind));
        out.push(
            (r.row_abs as u8)
                | ((r.col_abs as u8) << 1)
                | ((r.open_rows as u8) << 2)
                | ((r.open_cols as u8) << 3),
        );
        out.push(
            (r.r1.is_some() as u8)
                | ((r.c1.is_some() as u8) << 1)
                | ((r.r2.is_some() as u8) << 2)
                | ((r.c2.is_some() as u8) << 3)
                | ((r.anchor_row.is_some() as u8) << 4)
                | ((r.anchor_col.is_some() as u8) << 5),
        );
        out.push(0u8); // pad
        for v in [r.r1, r.c1, r.r2, r.c2, r.anchor_row, r.anchor_col] {
            out.extend_from_slice(&v.unwrap_or(0).to_le_bytes());
        }
        out.extend_from_slice(&(r.start as u32).to_le_bytes());
        out.extend_from_slice(&(r.end as u32).to_le_bytes());
        put_str(&mut out, &r.text);
        put_str(&mut out, r.sheet.as_deref().unwrap_or(""));
        put_str(&mut out, r.sheet_end.as_deref().unwrap_or(""));
        put_str(&mut out, r.anchor.as_deref().unwrap_or(""));
        put_str(&mut out, r.operator.as_deref().unwrap_or(""));
    }
    out
}

#[cfg(test)]
mod ref_tests {
    use super::parse_references;
    use formualizer_parse::FormulaDialect;

    fn refs(f: &str) -> Vec<super::CffiRef> {
        parse_references(f, FormulaDialect::Excel).unwrap()
    }

    #[test]
    fn cell_is_1based() {
        // Confirms the coordinate base: A1 => row 1, col 1 (not 0-based).
        let r = refs("=A1");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, "cell");
        assert_eq!((r[0].r1, r[0].c1, r[0].r2, r[0].c2), (Some(1), Some(1), Some(1), Some(1)));
        assert!(!r[0].row_abs && !r[0].col_abs);
    }

    #[test]
    fn absolute_flags_and_sheet() {
        let r = refs("=Sheet1!$C$2");
        assert_eq!(r[0].kind, "cell");
        assert_eq!(r[0].sheet.as_deref(), Some("Sheet1"));
        assert_eq!((r[0].r1, r[0].c1), (Some(2), Some(3)));
        assert!(r[0].row_abs && r[0].col_abs);
    }

    #[test]
    fn range_and_whole_column() {
        let r = refs("=SUM(A1:B10)");
        assert_eq!(r[0].kind, "range");
        assert_eq!((r[0].r1, r[0].c1, r[0].r2, r[0].c2), (Some(1), Some(1), Some(10), Some(2)));
        assert!(!r[0].open_rows && !r[0].open_cols);

        let w = refs("=SUM(A:A)");
        assert_eq!(w[0].kind, "range");
        assert!(w[0].open_rows, "A:A has open rows");
        assert_eq!((w[0].c1, w[0].c2), (Some(1), Some(1)));
        assert_eq!((w[0].r1, w[0].r2), (None, None));
    }

    #[test]
    fn reference_operators_are_normalized() {
        // spill (#), implicit intersection (@), a sheet-qualified cell, and a
        // percent operator (arithmetic — must NOT be read as a ref operator).
        let r = refs("=SUM(A1#, @B1:B5, Sheet1!C2, D1%)");
        let got: Vec<(&str, &str, Option<&str>)> = r
            .iter()
            .map(|x| (x.kind.as_str(), x.text.as_str(), x.anchor.as_deref()))
            .collect();
        assert_eq!(got[0], ("spill", "A1#", Some("A1")));
        assert_eq!(got[1], ("implicit_intersection", "@B1:B5", Some("B1:B5")));
        assert_eq!(got[2], ("cell", "Sheet1!C2", None));
        assert_eq!(got[3], ("cell", "D1", None));
        // The spill anchor carries a numeric origin for the anchor-identity join.
        assert_eq!((r[0].anchor_row, r[0].anchor_col), (Some(1), Some(1)));
    }

    #[test]
    fn packed_roundtrips_count_and_magic() {
        let r = refs("=A1+B2");
        let packed = super::pack_refs(&r);
        assert_eq!(&packed[0..4], &super::REFS_PACKED_MAGIC.to_le_bytes());
        let count = u32::from_le_bytes(packed[4..8].try_into().unwrap());
        assert_eq!(count as usize, r.len());
        assert_eq!(count, 2);
    }
}
