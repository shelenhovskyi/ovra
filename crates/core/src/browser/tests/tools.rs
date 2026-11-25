use serde_json::json;

use super::{fx, ok, open, pick, ready_web};
use crate::fakes::FAKE_PNG;
use crate::protocol::ErrorCode;
use crate::protocol::agents::SessionId;
use crate::protocol::browser::PageId;
use crate::protocol::projects::ProjectId;
use crate::services::{ElementRef, PageAction, ToolCaller, ToolRole};

fn caller(session: &str) -> ToolCaller {
    ToolCaller {
        session: SessionId(session.into()),
        project: ProjectId("p1".into()),
        role: ToolRole::Worker,
    }
}

#[tokio::test]
async fn the_tools_reach_the_callers_own_pages_only() {
    let f = fx(true).await;
    ready_web(&f).await;
    let me = caller("s1");
    let page = f
        .core
        .browser_open(&me, "http://localhost:5173/".into(), Some((800, 600)))
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .id;
    let away = PageAction::Navigate {
        url: "http://127.0.0.1:5173/b".into(),
    };
    let after = f.core.browser_act(&me, &page, away).await;
    assert_eq!(
        after.map(|p| p.url).ok().as_deref(),
        Some("http://127.0.0.1:5173/b")
    );

    let other = caller("s9");
    let refused = [
        f.core.browser_console(&other, &page, 0, 10).err(),
        f.core
            .browser_snapshot(&other, &page, None, None)
            .await
            .err(),
        f.core.browser_close(&other, &page).await.err(),
    ];
    for error in refused {
        assert_eq!(error.map(|e| e.code), Some(ErrorCode::NotFound));
    }
    assert!(f.core.browser_close(&me, &page).await.is_ok());
    let gone = f.core.browser_screenshot(&me, &page, None, false).await;
    assert_eq!(gone.err().map(|e| e.code), Some(ErrorCode::NotFound));
}

#[tokio::test]
async fn a_sent_pick_is_inspected_live_or_as_the_person_sent_it() {
    let f = fx(true).await;
    ready_web(&f).await;
    let page = open(&f.core, "http://localhost:5173/").await;
    let page = page.unwrap_or_else(|e| panic!("{e}")).id;
    *super::super::lock(&f.browser.element) = serde_json::from_value(pick("live", "")).ok();
    let attach = json!({"method": "browser.attach", "params": {"page": page, "picks": [pick("sent", "bigger")]}});
    ok(&f.core, attach).await;
    let send = json!({"method": "agents.send", "params": {"session": "s1", "text": "this"}});
    ok(&f.core, send).await;

    let me = caller("s1");
    let pk7 = || ElementRef::Pick("pk7".into());
    let live = f.core.browser_inspect(&me, None, pk7()).await;
    let live = live.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        (live.element.text.as_str(), live.element.note.as_str()),
        ("live", "bigger")
    );
    assert_eq!(live.png, FAKE_PNG);

    ok(
        &f.core,
        json!({"method": "browser.close", "params": {"page": page}}),
    )
    .await;
    let sent = f.core.browser_inspect(&me, None, pk7()).await;
    let sent = sent.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((sent.element.text.as_str(), sent.png.len()), ("sent", 0));
    let unknown = f
        .core
        .browser_inspect(&me, None, ElementRef::Pick("pk1".into()))
        .await;
    assert_eq!(unknown.err().map(|e| e.code), Some(ErrorCode::BadRequest));
    let other = f
        .core
        .browser_inspect(&caller("s9"), Some(PageId("pg1".into())), pk7())
        .await;
    assert_eq!(other.err().map(|e| e.code), Some(ErrorCode::NotFound));
}
