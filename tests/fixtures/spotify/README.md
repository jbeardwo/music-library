# Demon Days credit fixture

`demon-days-credits.json` records the 2026-09-23 fresh MusicBrainz catalog import
(Gorillaz — Demon Days, 2005, 15 Tracks) and the two Spotify US programs surviving
initial Album filtering. Captured by `album_resolution_probe`; the before/after
procedure and complete observations are in `docs/spotify-credit-matching-audit.md`.

Application credits retain MusicBrainz Artist IDs and original joins. Spotify
Artist IDs acquired by the diagnostic's later matching attempt are deliberately
excluded from the initial application evidence. Durations are unknown locally;
provider durations and occurrence IDs are retained. No credentials, user paths or
user-library state are included. The deterministic test performs no live requests.
