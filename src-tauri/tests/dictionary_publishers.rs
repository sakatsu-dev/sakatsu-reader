//! The explicit debug-only fixture feature replaces HTTP transport, while production
//! command registration, preparation, publication and reader adoption remain real.
#![cfg(all(debug_assertions, feature = "dictionary-publisher-tests"))]
#[test]
fn publisher_round_trip() {
    sakatsu_desktop_lib::dictionary_manager::publisher_tests::publisher_round_trip();
}
