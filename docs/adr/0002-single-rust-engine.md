# ADR 0002: One authoritative Rust engine

Status: accepted

The Rust `engine` crate is the only rules implementation. The web client renders exported authoritative frames and events and contains no game rules. The viewer's fog mask is a display filter over data a player is already entitled to. Balance values live in one versioned `RulesConfig`; a rules version is immutable once public matches use it.
