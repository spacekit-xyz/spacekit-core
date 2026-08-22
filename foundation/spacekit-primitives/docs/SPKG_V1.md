# SpaceKit Package Container (`.spkg`) v1

Status: initial interoperable container specification.

## Purpose

An SPKG is the immutable delivery unit for a user-facing SpaceKit artifact. It
contains a signed package manifest and all bytes needed to consume that
artifact. Storage indexes, DID documents, marketplace rows, and other internal
records are not SPKGs.

The v1 container is a ZIP archive with media type:

```text
application/vnd.spacekit.spkg+zip
```

## Required layout

```text
mimetype
manifest.json
payload/<package-relative path>
payload/<another package-relative path>
signatures/<optional detached signature>
```

- `mimetype` MUST be the first ZIP entry, MUST use the ZIP "stored" method, and
  MUST contain exactly `application/vnd.spacekit.spkg+zip` with no newline.
- `manifest.json` MUST contain a UTF-8 JSON `AppPackage` using schema
  `spacekit:app-package:v1`.
- Every `content_refs[].path` MUST have exactly one corresponding
  `payload/<path>` entry.
- Entries not listed above MUST be ignored unless a future manifest version
  marks them as required.

## Canonical path rules

Package paths use `/` separators and UTF-8 names. A path MUST NOT:

- be empty or absolute;
- contain `\`, NUL, an empty segment, `.` or `..`;
- name a directory;
- collide with another path.

Readers MUST apply these checks before writing an entry to a filesystem.

## Integrity

For each content reference:

- `size` is the uncompressed payload size;
- `hash` is SHA-256 of the uncompressed payload bytes;
- `compression` describes compression applied to the payload before it is put
  in the ZIP. For v1 producers it MUST be `None`; ZIP entry compression is a
  transport detail and is not represented by this field.

`manifest.checksum` retains the existing AppPackage convention:

```text
SHA-256(content_ref[0].hash || content_ref[1].hash || ...)
```

The ordered input values are the 32-byte binary hashes, not their hexadecimal
text. Readers MUST verify each payload hash and the aggregate checksum before
execution. A package with a missing, extra duplicate, or mismatched payload is
invalid.

The package identifier used by the package delivery API is:

```text
SHA-256(exact .spkg archive bytes)
```

It is encoded as 64 lowercase hexadecimal characters. Storage MUST return the
exact uploaded archive bytes for that identifier.

## Deterministic production

To make archive identifiers reproducible, producers MUST:

1. serialize `manifest.json` using a stable representation;
2. write `mimetype`, then `manifest.json`, then payload entries sorted by path;
3. use fixed ZIP timestamps, permissions, and flags;
4. omit platform-specific extra fields and directory entries.

Changing ZIP compression parameters changes the package identifier even when
the payload is unchanged. Producers SHOULD use a single documented compression
profile.

## Resource limits and safe extraction

Conforming readers and storage nodes MUST reject:

- more than 10,000 archive entries;
- more than 512 MiB total declared uncompressed data;
- encrypted ZIP entries;
- unsafe or duplicate paths;
- unsupported compression methods;
- payload sizes or hashes that differ from the manifest.

Implementations MAY impose stricter limits. Validation SHOULD stream entry
hashing and SHOULD avoid extracting the archive to disk.

## Encryption

SPKG defines packaging and integrity, not recipient encryption. When encrypted
delivery is required, the complete SPKG bytes are the plaintext carried by the
SpaceKit PQ envelope. Consumers decrypt the envelope, verify the SPKG archive,
then open its payload.

## HTTP delivery

Storage nodes expose immutable archives as:

```text
PUT  /packages/{sha256}
GET  /packages/{sha256}
HEAD /packages/{sha256}
```

Successful reads use `Content-Type: application/vnd.spacekit.spkg+zip`,
an immutable cache policy, and an ETag derived from the package identifier.

## Compatibility

During migration, clients MAY continue to read legacy AppPackage manifests
whose payloads are separate FactPackages. Producers MUST create real SPKG
archives. Deploy tooling MAY additionally publish exploded Facts for older
runtimes, but those Facts are compatibility projections rather than the
canonical delivery artifact.
