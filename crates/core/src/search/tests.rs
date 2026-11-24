use std::path::Path;
use std::sync::Arc;

use crate::fakes::{MemoryFs, MemoryProjects};
use crate::protocol::projects::{ProjectId, ProjectsRequest, ProjectsResponse};
use crate::protocol::search::{SearchId, SearchQuery, SearchRequest, SearchResponse};
use crate::protocol::{ErrorCode, Event, Request, Response};
use crate::{Core, test_path};

const ROOT: &str = test_path!("/work/a");

async fn fixture() -> (Core, Arc<MemoryFs>, ProjectId) {
    let fs = Arc::new(MemoryFs::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .build();
    let added = core
        .handle(Request::Projects(ProjectsRequest::Add {
            root: ROOT.into(),
        }))
        .await;
    let Ok(Response::Projects(ProjectsResponse::Project(project))) = added else {
        panic!("{added:?}");
    };
    (core, fs, project.id)
}

fn query(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        regex: false,
        case_sensitive: false,
        whole_word: false,
        include: vec![],
        exclude: vec![],
    }
}

async fn start(core: &Core, project: &ProjectId, q: SearchQuery) -> Result<SearchId, ErrorCode> {
    let request = Request::Search(SearchRequest::Start {
        project: project.clone(),
        query: q,
    });
    match core.handle(request).await {
        Ok(Response::Search(SearchResponse::Started { id })) => Ok(id),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e.code),
    }
}

#[tokio::test]
async fn results_stream_as_events_and_a_done_ends_them() {
    let (core, fs, project) = fixture().await;
    fs.put(
        Path::new(ROOT),
        "src/main.rs",
        "fn main() {\n    println!(\"Hello\");\n}\n",
    );
    fs.put(Path::new(ROOT), "README.md", "Say hello, héllo, hello.\n");
    fs.put(Path::new(ROOT), "empty.txt", "nothing here\n");
    let mut events = core.subscribe();
    let id = start(&core, &project, query("hello")).await;
    assert_eq!(id, Ok(SearchId(1)));
    let events: Vec<Event> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    insta::assert_json_snapshot!(events);
}

#[tokio::test]
async fn ids_count_up() {
    let (core, _fs, project) = fixture().await;
    assert_eq!(start(&core, &project, query("a")).await, Ok(SearchId(1)));
    assert_eq!(start(&core, &project, query("b")).await, Ok(SearchId(2)));
}

#[tokio::test]
async fn an_empty_or_huge_pattern_is_a_bad_request() {
    let (core, fs, project) = fixture().await;
    assert_eq!(
        start(&core, &project, query("")).await,
        Err(ErrorCode::BadRequest)
    );
    let huge = "x".repeat(1001);
    assert_eq!(
        start(&core, &project, query(&huge)).await,
        Err(ErrorCode::BadRequest)
    );
    assert!(
        fs.searches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    );
}

#[tokio::test]
async fn a_query_the_service_rejects_is_answered_with_its_error() {
    let (core, _fs, project) = fixture().await;
    let mut q = query("a");
    q.regex = true;
    assert_eq!(start(&core, &project, q).await, Err(ErrorCode::BadRequest));
    assert!(core.searches().running().is_empty());
}

#[tokio::test]
async fn an_unknown_project_is_not_found() {
    let (core, _fs, _) = fixture().await;
    let other = ProjectId("nope".into());
    assert_eq!(
        start(&core, &other, query("a")).await,
        Err(ErrorCode::NotFound)
    );
}

#[tokio::test]
async fn cancel_sets_the_flag_and_answers_ok_for_ended_searches_too() {
    let (core, fs, project) = fixture().await;
    let id = start(&core, &project, query("a"))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let cancel = |id| core.handle(Request::Search(SearchRequest::Cancel { id }));
    assert!(cancel(id).await.is_ok());
    let flags = fs
        .searches
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert!(flags[0].is_cancelled());
    assert!(cancel(id).await.is_ok());
    assert!(cancel(SearchId(99)).await.is_ok());
}

#[tokio::test]
async fn flags_of_ended_searches_are_dropped() {
    let (core, fs, project) = fixture().await;
    for pattern in ["a", "b", "c"] {
        let _ = start(&core, &project, query(pattern)).await;
        // The fake keeps every flag; letting go of them ends the searches.
        fs.searches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    let _ = start(&core, &project, query("d")).await;
    assert_eq!(core.searches().running().len(), 1);
}

#[tokio::test]
async fn starting_a_fifth_search_cancels_the_oldest() {
    let (core, fs, project) = fixture().await;
    for pattern in ["a", "b", "c", "d", "e"] {
        let _ = start(&core, &project, query(pattern)).await;
    }
    let flags = fs
        .searches
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let cancelled: Vec<bool> = flags.iter().map(|f| f.is_cancelled()).collect();
    assert_eq!(cancelled, [true, false, false, false, false]);
    assert_eq!(core.searches().running().len(), 4);
}
