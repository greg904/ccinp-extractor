use std::collections::HashSet;
use std::convert::Infallible;
use std::convert::TryFrom;
use std::hash::Hash;
use std::io::Write;
use std::net::SocketAddr;
use std::str;
use std::sync::{Arc, Mutex};

use clap::Parser;

use hyper::service::{make_service_fn, service_fn};
use hyper::{http, StatusCode};
use hyper::{Body, Response, Server};

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfObject;

enum FilterAction {
    KeepKid,
    RemoveKid,
    Subtree { subtract_count: i32 },
}

fn filter_page_tree_helper<F: FnMut(&PdfObject) -> bool>(
    mut node: PdfObject,
    f: &mut F,
) -> Result<FilterAction, mupdf::Error> {
    let ty = match node
        .get_dict("Type")
        .ok()
        .flatten()
        .and_then(|t| t.as_name().ok().map(|s| s.to_owned()))
    {
        Some(val) => val,
        None => return Ok(FilterAction::Subtree { subtract_count: 0 }),
    };
    match &*ty {
        b"Page" => Ok(if f(&node) {
            FilterAction::KeepKid
        } else {
            FilterAction::RemoveKid
        }),
        b"Pages" => {
            let mut subtract_count = 0;
            if let Some(mut kids) = node.get_dict("Kids").unwrap() {
                let mut kids_len = i32::try_from(kids.len().unwrap()).unwrap();
                let mut i: i32 = 0;
                while i < kids_len {
                    let kid = kids.get_array(i).unwrap().unwrap();
                    match filter_page_tree_helper(kid, f)? {
                        FilterAction::KeepKid => {}
                        FilterAction::RemoveKid => {
                            kids.array_delete(i).unwrap();
                            kids_len -= 1;
                            subtract_count += 1;
                            continue;
                        }
                        FilterAction::Subtree { subtract_count: n } => subtract_count += n,
                    }
                    i += 1;
                }
                if subtract_count > 0 {
                    let count = node.get_dict("Count").unwrap().unwrap().as_int().unwrap();
                    node.dict_put("Count", PdfObject::new_int(count - subtract_count).unwrap())
                        .unwrap();
                }
            }
            Ok(FilterAction::Subtree { subtract_count })
        }
        _ => panic!("invalid type in page tree"),
    }
}

fn filter_page_tree<F: FnMut(&PdfObject) -> bool>(
    root: PdfObject,
    mut f: F,
) -> Result<(), mupdf::Error> {
    filter_page_tree_helper(root, &mut f)?;
    Ok(())
}

pub struct ExerciseExtractor<'a> {
    doc_bytes: &'a [u8],
}

pub enum ExtractError {
    Mupdf(mupdf::Error),
    InvalidDoc,
    MissingExercise,
}

impl<'a> ExerciseExtractor<'a> {
    pub fn new(doc_bytes: &'a [u8]) -> Self {
        Self { doc_bytes }
    }

    fn read_exercise_number(page: &PdfObject) -> Option<i32> {
        let contents = match page.get_dict("Contents").ok().flatten() {
            Some(val) => val,
            None => return None,
        };
        let stream = match contents.read_stream() {
            Ok(val) => val,
            Err(_) => return None,
        };
        let stream_str = match str::from_utf8(&stream) {
            Ok(val) => val,
            Err(_) => return None,
        };
        let markers = ["(EXER)31(CICE)", "(Exercice)"];
        for marker in markers.iter() {
            if let Some(i) = stream_str.find(marker) {
                if let Some(l_paren) = stream_str[i + marker.len()..].find('(') {
                    let l_paren = (i + marker.len()) + l_paren;
                    if let Some(r_paren) = stream_str[(l_paren + 1)..].find(')') {
                        let r_paren = (l_paren + 1) + r_paren;
                        match stream_str[(l_paren + 1)..r_paren].parse() {
                            Ok(val) => return Some(val),
                            Err(_) => {},
                        }
                    }
                }
            }
        }
        None
    }

    pub fn extract<W: Write>(
        &self,
        exercise_numbers: &[i32],
        w: &mut W,
    ) -> Result<(), ExtractError> {
        let doc = PdfDocument::from_bytes(&self.doc_bytes).map_err(ExtractError::Mupdf)?;
        let catalog_id = doc.catalog().map_err(ExtractError::Mupdf)?;
        let mut catalog = catalog_id
            .resolve()
            .map_err(ExtractError::Mupdf)?
            .ok_or(ExtractError::InvalidDoc)?;
        let tree_id = catalog
            .get_dict("Pages")
            .map_err(ExtractError::Mupdf)?
            .ok_or(ExtractError::InvalidDoc)?;
        let tree = tree_id
            .resolve()
            .map_err(ExtractError::Mupdf)?
            .ok_or(ExtractError::InvalidDoc)?;
        let mut current_exercise_number: i32 = -1;
        let mut not_seen = exercise_numbers.to_vec();
        filter_page_tree(tree, |page: &PdfObject| {
            if let Some(n) = Self::read_exercise_number(page) {
                current_exercise_number = n;
                if let Some(pos) =
                    not_seen.iter().position(|it| *it == n)
                {
                    not_seen.swap_remove(pos);
                }
            }
            exercise_numbers.contains(&current_exercise_number)
        })
        .map_err(ExtractError::Mupdf)?;
        if !not_seen.is_empty() {
            return Err(ExtractError::MissingExercise);
        }
        catalog
            .dict_delete("Outlines")
            .map_err(ExtractError::Mupdf)?;
        doc.write_to(w).map_err(ExtractError::Mupdf)?;
        Ok(())
    }
}

fn has_duplicate_elements<T>(iter: T) -> bool
where
    T: IntoIterator,
    T::Item: Eq + Hash,
{
    let mut uniq = HashSet::new();
    iter.into_iter().any(move |x| !uniq.insert(x))
}

fn not_found() -> http::Result<http::Response<Body>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain")
        .body(Body::from("Not found"))
}

fn internal_server_error() -> http::Result<http::Response<Body>> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("Content-Type", "text/plain")
        .body(Body::from("Internal server error"))
}

const EXERCISES_DOCUMENT: &[u8; 966410] = include_bytes!("exercises.pdf");

/// An HTTP server that serves parts of the CCINP exercise document
#[derive(Parser)]
#[clap()]
struct Args {
    /// The address to bind to
    #[clap(short, long, default_value = "127.0.0.1:3000")]
    addr: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    let addr: SocketAddr = args.addr.parse()?;

    let exercise_extractor = Arc::new(Mutex::new(ExerciseExtractor::new(EXERCISES_DOCUMENT)));

    // For every connection, we must make a `Service` to handle all
    // incoming HTTP requests on said connection.
    let make_svc = make_service_fn(|_conn| {
        let exercise_extractor = exercise_extractor.clone();
        // This is the `Service` that will handle the connection.
        // `service_fn` is a helper to convert a function that
        // returns a Response into a `Service`.
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let exercise_extractor = exercise_extractor.clone();
                async move {
                    let path = req.uri().path();
                    if !path.starts_with('/') {
                        return not_found();
                    }
                    let path_without_pdf = path.strip_suffix(".pdf").unwrap_or(path);
                    let exercise_numbers = match path_without_pdf[1..]
                        .split(',')
                        .map(|p| p.parse())
                        .collect::<Result<Vec<i32>, _>>()
                    {
                        Ok(val) => val,
                        Err(_) => return not_found(),
                    };
                    if has_duplicate_elements(exercise_numbers.iter()) {
                        return not_found();
                    }
                    let res = {
                        let mut tmp = Vec::new();
                        let e = match exercise_extractor.lock() {
                            Ok(val) => val,
                            Err(_) => return internal_server_error(),
                        };
                        match e.extract(&exercise_numbers, &mut tmp) {
                            Err(ExtractError::MissingExercise) => return not_found(),
                            Err(_) => return internal_server_error(),
                            _ => tmp,
                        }
                    };
                    Response::builder()
                        .header("Content-Type", "application/pdf")
                        .body(Body::from(res))
                }
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Listening on http://{}", addr);

    server.await?;

    Ok(())
}
