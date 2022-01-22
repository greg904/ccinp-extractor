use std::str;

use lopdf::{Document, Object, content::Operation};

fn match_string_object(obj: &Object, expected: &str) -> bool {
    match obj {
        Object::String(s, _format) => expected.bytes().eq(s.iter().cloned()),
        _ => false,
    }
}

fn read_exercise_number_from_operation(op: &Operation) -> Option<i32> {
    if op.operator != "TJ" || op.operands.len() < 1 {
        return None;
    }
    let text = match &op.operands[0] {
        Object::Array(arr) => arr,
        _ => return None,
    };
    if text.len() < 5 ||
        !match_string_object(&text[0], "EXER") ||
        !match_string_object(&text[2], "CICE") {
        return None;
    }
    match &text[4] {
        Object::String(s, _format) =>
            str::from_utf8(s).ok()?.parse().ok(),
        _ => None,
    }
}

fn main() {
    let mut doc = Document::load("exercises.pdf").expect("failed to load PDF");
    let mut current_exercise_number = -1;
    for (_page_number, page_id) in doc.get_pages() {
        let content = doc.get_and_decode_page_content(page_id).expect("failed to decode page content");
        match content.operations.iter().find_map(read_exercise_number_from_operation) {
            Some(val) => current_exercise_number = val,
            None => {},
        }
        if current_exercise_number != 105 {
            if let Some(page) = doc.get_object(page_id).ok() {
                let mut page_tree_ref = page
                    .as_dict()
                    .and_then(|dict| dict.get(b"Parent"))
                    .and_then(Object::as_reference);
                while let Ok(page_tree_id) = page_tree_ref {
                    if let Some(page_tree) = doc.get_object_mut(page_tree_id).ok().and_then(|pt| pt.as_dict_mut().ok()) {
                        if let Ok(count) = page_tree.get(b"Count").and_then(Object::as_i64) {
                            page_tree.set("Count", count - 1);
                        }
                        page_tree_ref = page_tree.get(b"Parent").and_then(Object::as_reference);
                    } else {
                        break;
                    }
                }
            }
            doc.delete_object(page_id);
        }
    }
    doc.save("exercises-extracted.pdf").expect("failed to save PDF");
}
