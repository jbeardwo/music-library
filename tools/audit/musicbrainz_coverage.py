#!/usr/bin/env python3
"""Read-only source -> SQLite backup -> bounded-scope MusicBrainz audit (no network).
Usage: python3 tools/audit/musicbrainz_coverage.py SOURCE /tmp/mb-audit-NAME.sqlite
Reports attached identities, not independent verification of their correctness.
"""
import collections
import json
import pathlib
import sqlite3
import sys


def measure(c):
    c.row_factory = sqlite3.Row
    c.executescript("""
    CREATE TEMP VIEW audit_track AS
      SELECT t.*,r.album_id FROM library_membership m JOIN track t ON t.id=m.track_id
      JOIN release r ON r.id=t.release_id;
    CREATE TEMP VIEW audit_album AS SELECT DISTINCT album_id FROM audit_track;
    CREATE TEMP VIEW audit_artist AS
      SELECT artist_id FROM track_artist_credit WHERE track_id IN(SELECT id FROM audit_track)
      UNION SELECT artist_id FROM release_artist_credit WHERE release_id IN(SELECT release_id FROM audit_track)
      UNION SELECT artist_id FROM album_artist_credit WHERE album_id IN(SELECT album_id FROM audit_album);
    CREATE TEMP VIEW audit_connected AS SELECT t.id FROM audit_track t WHERE EXISTS(
      SELECT 1 FROM recording_external_identity i WHERE i.recording_id=t.recording_id
      AND provider='musicbrainz' AND kind='recording' AND length(external_id)>0);
    """)
    scalar = lambda q: c.execute(q).fetchone()[0]
    report = {'user_version': scalar('PRAGMA user_version')}
    report['artists'] = dict(c.execute("""SELECT count(*) total, sum(EXISTS(SELECT 1 FROM
      artist_external_identity i WHERE i.artist_id=a.artist_id AND provider='musicbrainz'
      AND kind='artist' AND length(external_id)>0)) connected FROM audit_artist a""").fetchone())
    report['albums'] = dict(c.execute("""SELECT count(*) total,
      sum(EXISTS(SELECT 1 FROM album_external_identity i WHERE i.album_id=a.album_id
       AND provider='musicbrainz' AND kind='release_group' AND length(external_id)>0)) release_group,
      sum(EXISTS(SELECT 1 FROM audit_track t JOIN release_external_identity i ON i.release_id=t.release_id
       WHERE t.album_id=a.album_id AND provider='musicbrainz' AND kind='release' AND length(external_id)>0)) exact_release
      FROM audit_album a""").fetchone())
    report['tracks'] = {'total': scalar('SELECT count(*) FROM audit_track'),
                        'connected': scalar('SELECT count(*) FROM audit_connected')}
    report['tracks']['missing'] = report['tracks']['total'] - report['tracks']['connected']
    report['tracks']['missing_with_release_group'] = scalar("""SELECT count(*) FROM audit_track t
      WHERE id NOT IN(SELECT id FROM audit_connected) AND EXISTS(SELECT 1 FROM album_external_identity i
      WHERE i.album_id=t.album_id AND provider='musicbrainz' AND kind='release_group')""")
    report['tracks']['missing_with_available_local_source'] = scalar("""SELECT count(*) FROM audit_track t
      WHERE id NOT IN(SELECT id FROM audit_connected) AND EXISTS(SELECT 1 FROM track_source s JOIN
      local_file_observation l ON l.source_id=s.source_id WHERE s.track_id=t.id AND l.available=1
      AND NOT EXISTS(SELECT 1 FROM local_source_suppression x WHERE x.source_id=s.source_id))""")
    report['unresolved_albums'] = []
    for a in c.execute("""SELECT a.album_id,a.title,a.match_artist_credit artist FROM effective_album_metadata a
      JOIN audit_album b ON b.album_id=a.album_id ORDER BY a.match_artist_credit,a.title""").fetchall():
        tracks = [dict(r) for r in c.execute("""SELECT t.id,e.title,t.id IN(SELECT id FROM audit_connected) connected,
          t.disc_number,t.track_number,e.duration_ms FROM audit_track t JOIN effective_track_metadata e
          ON e.track_id=t.id WHERE album_id=? ORDER BY t.disc_number,t.track_number,t.id""", (a['album_id'],))]
        ids = [dict(r) for r in c.execute("SELECT kind,external_id FROM album_external_identity WHERE album_id=? AND provider='musicbrainz'", (a['album_id'],))]
        if any(not t['connected'] for t in tracks) or not ids:
            report['unresolved_albums'].append(dict(a, identities=ids, tracks=tracks))
    report['conflicts'] = {}
    report['provenance'] = {}
    for entity, kind in [('artist','artist'), ('album','release_group'), ('release','release'), ('recording','recording')]:
        key = entity + '_id'
        scope = {'artist':'SELECT artist_id FROM audit_artist', 'album':'SELECT album_id FROM audit_album',
                 'release':'SELECT release_id FROM audit_track','recording':'SELECT recording_id FROM audit_track'}[entity]
        report['conflicts'][entity] = [dict(r) for r in c.execute(f"""SELECT {key},group_concat(external_id) ids,count(*) count
          FROM {entity}_external_identity WHERE provider='musicbrainz' AND kind=? AND {key} IN({scope}) GROUP BY {key} HAVING count(*)>1""", (kind,))]
        if entity in ('album', 'recording'):
            report['provenance'][entity] = dict(c.execute(f"""SELECT count(*) identities,
              sum(EXISTS(SELECT 1 FROM {entity}_provenance_identity p WHERE p.{key}=i.{key}
              AND p.provider=i.provider AND p.kind=i.kind AND p.external_id=i.external_id)) managed_embedded
              FROM {entity}_external_identity i WHERE provider='musicbrainz' AND kind=? AND {key} IN({scope})""", (kind,)).fetchone())
    embedded = collections.defaultdict(set)
    raw_conflicts = []
    for row in c.execute("""SELECT t.recording_id,r.album_id,f.provenance_json FROM audit_track t
      JOIN release r ON r.id=t.release_id JOIN track_source s ON s.track_id=t.id JOIN
      file_metadata_observation f ON f.source_id=s.source_id WHERE f.provenance_json IS NOT NULL"""):
        for o in json.loads(row['provenance_json'])['observation']['observations']:
            i = o['identity']
            entity = {'release_group':'album','recording':'recording'}.get(i['kind'])
            if i['provider'] != 'musicbrainz' or entity is None or 'embedded_tag' not in o['origin']:
                continue
            eid = row[entity+'_id']
            attached = [r[0] for r in c.execute(f"SELECT external_id FROM {entity}_external_identity WHERE {entity}_id=? AND provider='musicbrainz' AND kind=?", (eid,i['kind']))]
            if i['external_id'] in attached:
                embedded[entity].add((eid,i['external_id']))
            elif attached:
                raw_conflicts.append((entity,eid,i['external_id'],tuple(attached)))
    report['matching_embedded_observations'] = {k:len(v) for k,v in embedded.items()}
    report['embedded_vs_attached_conflicts'] = sorted(set(raw_conflicts))
    report['manual_recording_markers'] = scalar("""SELECT count(*) FROM recording_manual_identity WHERE
      provider='musicbrainz' AND kind='recording' AND recording_id IN(SELECT recording_id FROM audit_track)""")
    assert report['tracks']['connected'] + report['tracks']['missing'] == report['tracks']['total']
    assert sum(sum(not t['connected'] for t in a['tracks']) for a in report['unresolved_albums']) == report['tracks']['missing']
    return report


if __name__ == '__main__':
    source, target = sys.argv[1:]
    if not target.startswith('/tmp/mb-audit-') or pathlib.Path(target).exists():
        raise SystemExit('Use a NEW /tmp/mb-audit-*.sqlite backup path')
    original = sqlite3.connect(pathlib.Path(source).resolve().as_uri()+'?mode=ro', uri=True)
    copy = sqlite3.connect(target)
    original.backup(copy)
    assert copy.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
    print(json.dumps(dict(source=source, backup=target, **measure(copy)), indent=2, ensure_ascii=False))
