// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;
mod support;

use helper::COL_ID;
use support::{
    REMOTE_CALENDAR_FOLDER, REMOTE_CALENDAR_NAME, REMOTE_CALENDAR2_FOLDER, REMOTE_CALENDAR2_NAME,
    RadicalePair, check_requirements,
};

#[tokio::test]
async fn discover_collection_succeeds() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let producer_cal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    assert!(!producer_cal.is_empty());

    let json = pair.consumer().discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert!(json["date"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        pair.consumer()
            .calendar_dir(REMOTE_CALENDAR_FOLDER)
            .exists(),
        "expected discover to create local vdirsyncer calendar directory"
    );
}

#[tokio::test]
async fn add_calendar_creates_remote_calendar_visible_to_other_peer() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let observer = pair.spawn_peer().await;
    assert!(
        !observer.calendar_dir(REMOTE_CALENDAR_FOLDER).exists(),
        "observer cache should not exist before addcal test"
    );

    let consumer_cal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    assert!(!consumer_cal.is_empty());

    let json = observer.discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert!(
        observer.calendar_dir(REMOTE_CALENDAR_FOLDER).exists(),
        "expected discover to cache the remotely created calendar"
    );
}

#[tokio::test]
async fn add_calendar_fails_for_read_only_collection() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(true, false).await;

    let (status, body) = pair
        .consumer()
        .try_create_calendar(REMOTE_CALENDAR_NAME)
        .await;
    assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.is_empty(), "unexpected body:\n{body}");
    assert!(
        !pair
            .consumer()
            .calendar_dir(REMOTE_CALENDAR_FOLDER)
            .exists(),
        "read-only addcal must not create a local cache directory"
    );
}

#[tokio::test]
async fn sync_collection_pulls_remote_event_into_store() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let producer_cal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let consumer_cal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;

    pair.producer()
        .create_and_sync_todo(&producer_cal, "Synced Event")
        .await
        .unwrap();

    let json = pair.consumer().sync_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert_eq!(json["calendars"][consumer_cal], false);

    let uid = pair.producer().uid_for_summary("Synced Event").await;
    pair.consumer()
        .assert_store_summary(&uid, "Synced Event")
        .await;
}

#[tokio::test]
async fn sync_all_pulls_remote_event_into_store() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let producer_cal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let consumer_cal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;

    pair.producer()
        .create_and_sync_todo(&producer_cal, "Sync All Event")
        .await
        .unwrap();

    let json = pair.consumer().sync_all().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert_eq!(json["calendars"][consumer_cal], false);

    let uid = pair.producer().uid_for_summary("Sync All Event").await;
    pair.consumer()
        .assert_store_summary(&uid, "Sync All Event")
        .await;
}

#[tokio::test]
async fn reload_collection_refreshes_local_cache_from_remote_server() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let producer_cal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let consumer_cal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;

    pair.producer()
        .create_and_sync_todo(&producer_cal, "Remote Collection Summary")
        .await
        .unwrap();
    let uid = pair
        .producer()
        .uid_for_summary("Remote Collection Summary")
        .await;

    pair.consumer().discover_collection().await;
    pair.consumer().sync_collection().await;
    pair.consumer()
        .overwrite_cached_todo(REMOTE_CALENDAR_FOLDER, &uid, "Locally Modified Summary");

    let json = pair.consumer().reload_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert_eq!(json["calendars"][consumer_cal], false);

    pair.consumer()
        .assert_store_summary(&uid, "Remote Collection Summary")
        .await;
}

#[tokio::test]
async fn reload_calendar_refreshes_only_the_selected_calendar() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let producer_work = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let consumer_work = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let producer_personal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR2_NAME, REMOTE_CALENDAR2_FOLDER)
        .await;
    let consumer_personal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR2_NAME, REMOTE_CALENDAR2_FOLDER)
        .await;

    pair.producer()
        .create_and_sync_todo(&producer_work, "Remote Work Summary")
        .await
        .unwrap();
    pair.producer()
        .create_and_sync_todo(&producer_personal, "Remote Personal Summary")
        .await
        .unwrap();

    let work_uid = pair.producer().uid_for_summary("Remote Work Summary").await;
    let personal_uid = pair
        .producer()
        .uid_for_summary("Remote Personal Summary")
        .await;

    pair.consumer().discover_collection().await;
    pair.consumer().sync_collection().await;
    pair.consumer().overwrite_cached_todo(
        REMOTE_CALENDAR_FOLDER,
        &work_uid,
        "Locally Modified Work",
    );
    pair.consumer().overwrite_cached_todo(
        REMOTE_CALENDAR2_FOLDER,
        &personal_uid,
        "Locally Modified Personal",
    );

    let json = pair.consumer().reload_calendar(&consumer_work).await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert_eq!(json["calendars"][consumer_work], false);
    assert_eq!(json["calendars"][consumer_personal], false);

    pair.consumer()
        .assert_store_summary(&work_uid, "Remote Work Summary")
        .await;
    pair.consumer()
        .assert_store_summary(&personal_uid, "Locally Modified Personal")
        .await;
}

#[tokio::test]
async fn delete_calendar_by_folder_removes_remote_calendar_for_other_peer() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(false, false).await;
    let observer_before_delete = pair.spawn_peer().await;
    let producer_cal = pair
        .producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;
    let consumer_cal = pair
        .consumer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;

    pair.producer()
        .create_and_sync_todo(&producer_cal, "Calendar To Delete")
        .await
        .unwrap();
    let uid = pair.producer().uid_for_summary("Calendar To Delete").await;

    pair.consumer().discover_collection().await;
    pair.consumer().sync_collection().await;
    pair.consumer()
        .assert_store_summary(&uid, "Calendar To Delete")
        .await;

    let json = observer_before_delete.discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert!(
        observer_before_delete
            .calendar_dir(REMOTE_CALENDAR_FOLDER)
            .exists(),
        "observer should see the remote calendar before deletion"
    );

    pair.consumer()
        .delete_calendar_by_folder(REMOTE_CALENDAR_FOLDER)
        .await;

    let observer_after_delete = pair.spawn_peer().await;
    let json = observer_after_delete.discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert!(
        !observer_after_delete
            .calendar_dir(REMOTE_CALENDAR_FOLDER)
            .exists(),
        "fresh observer should not discover the deleted remote calendar"
    );

    let json = pair.consumer().discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert_eq!(json["calendars"][consumer_cal], false);
    assert!(
        !pair
            .consumer()
            .calendar_dir(REMOTE_CALENDAR_FOLDER)
            .exists(),
        "consumer local cache should be removed for the deleted calendar"
    );
    pair.consumer().assert_store_missing(&uid).await;
}

#[tokio::test]
async fn delete_calendar_by_folder_fails_for_read_only_collection() {
    if let Some(msg) = check_requirements() {
        eprintln!("{msg}");
        return;
    }

    let pair = RadicalePair::new(true, false).await;
    pair.producer()
        .create_calendar(REMOTE_CALENDAR_NAME, REMOTE_CALENDAR_FOLDER)
        .await;

    let (status, body) = pair
        .consumer()
        .try_delete_calendar_by_folder(REMOTE_CALENDAR_FOLDER)
        .await;
    assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.is_empty(), "unexpected body:\n{body}");

    let observer = pair.spawn_peer().await;
    let json = observer.discover_collection().await;
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );
    assert!(
        observer.calendar_dir(REMOTE_CALENDAR_FOLDER).exists(),
        "read-only delete must leave the remote calendar intact"
    );
}
