# Commands

The image's entrypoint is `/practical`; RustiCal's binary sits at `/usr/local/bin/rustical`.

## PractiCal

```text
practical serve                            # default: the web UI
practical check-rustical-image <ref>       # exit 0 iff the image tag == the compiled rustical crate tag
practical gen-vapid                        # print a fresh VAPID private key (PEM) + public key (Web Push)
practical --version                        # "practical X.Y.Z (rustical crates vA.B.C)"
```

`GET /healthz` answers `200 ok` once the database is reachable (readiness/liveness probe). It needs no session.

## RustiCal (user and group administration)

Users, groups and memberships are RustiCal principals, managed with RustiCal's CLI inside the `rustical` container
(same image, same database), for example on Kubernetes:

```bash
R="kubectl exec -it deploy/practical -c rustical -- rustical"
$R principals create alice -n "Alice" --password             # a user; prompts for the password
$R principals create group.family -p group -n "Family"       # a group
$R principals membership assign alice --to group.family     # alice may use the group's calendars
$R principals --help                                         # everything else
```

`--password` asks once (`Type in a password:`). An interrupted prompt can leave the principal created with an unknown
password: set it again with `--overwrite`. Calendar apps never use this password — DAV accepts only app tokens
(PractiCal settings → app tokens, or `rustical principals app-token create --name <device> <id>`).

Day-to-day group work (new groups, members, calendars, address books, app tokens, the trash) is done in
PractiCal's settings; the CLI is needed for the first user.
