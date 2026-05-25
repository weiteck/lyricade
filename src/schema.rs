// @generated automatically by Diesel CLI.

diesel::table! {
    libraries (id) {
        id -> Integer,
        path -> Text,
        name -> Nullable<Text>,
        added_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    settings (id) {
        id -> Integer,
        prefer_accurate_timestamps -> Bool,
        prefer_lyrics_type -> Text,
        scan_new_files_only -> Bool,
        ignore_plain_lyrics_on_fetch -> Bool,
        update_lyrics_tag_on_fetch -> Bool,
        save_sidecar_file_on_fetch -> Bool,
        get_lyrics_menu_lyrics_type -> Text,
        get_lyrics_menu_last_checked -> Text,
        get_lyrics_menu_filtered -> Bool,
        get_lyrics_menu_selected -> Bool,
        window_width -> Integer,
        window_height -> Integer,
        sidebar_pinned -> Bool,
        added_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    tracks (id) {
        id -> Integer,
        library_id -> Integer,
        path -> Text,
        track_name -> Text,
        artist_name -> Text,
        album_name -> Text,
        duration -> Float,
        instrumental -> Nullable<Bool>,
        lyrics -> Nullable<Text>,
        lyrics_synchronised -> Bool,
        lyrics_sidecar_lrc_file -> Nullable<Text>,
        lyrics_sidecar_txt_file -> Nullable<Text>,
        added_at -> Timestamp,
        updated_at -> Timestamp,
        refreshed_at -> Timestamp,
        last_api_check_at -> Nullable<Timestamp>,
        file_modified_at -> Timestamp,
    }
}

diesel::joinable!(tracks -> libraries (library_id));

diesel::allow_tables_to_appear_in_same_query!(
    libraries,
    settings,
    tracks,
);
