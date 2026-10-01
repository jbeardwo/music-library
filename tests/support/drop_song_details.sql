DROP TABLE track_duration_observation;
DROP INDEX song_details_artist;
DROP INDEX song_details_album;
DROP INDEX song_details_genre;
ALTER TABLE effective_track_metadata DROP COLUMN duration_approximate;
ALTER TABLE effective_track_metadata DROP COLUMN genre_names;
