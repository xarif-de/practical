#!/usr/bin/env bash
# A throwaway Nextcloud for developing the CalDAV import and sync, in the version
# plan.xarif.de's Nextcloud runs (35.0.1, `status.php` of my.xarif.de on 2026-10-10).
#
# It holds every shape the code has to tell apart:
#   alice (the user we import/sync as)
#     work              own, plain events
#     mixed             own, one event per CLASS (PUBLIC, PRIVATE, CONFIDENTIAL) -> an OWN
#                       calendar hands all three out in full; the shared one below does not
#     repeating         own, a weekly event with one moved instance (RECURRENCE-ID) ->
#                       master and override are ONE object
#     holiday-feed      a webcal subscription -> must never be offered
#     (trashed)         a deleted calendar -> must never be offered
#     contacts          own address book, one card with a birthday -> makes Nextcloud
#                       generate the derived "contact_birthdays" calendar
#   bob
#     bobs-plan         shared TO alice, one event per CLASS -> proves why a shared
#                       calendar is refused: PRIVATE vanishes, CONFIDENTIAL is gutted
#     bobs-contacts     address book shared to alice
#
# Credentials are development-only and intentionally in the clear here.
set -euo pipefail

NC="${NC:-http://localhost:8099}"
CT="${CT:-practical-nc}"
IMG="${IMG:-nextcloud:35.0.1-apache}"
ADMIN_PW="adminpw-dev-only"
USER_PW="alice-dev-only-pw"

occ() { docker exec -u www-data -e OC_PASS="${OC_PASS:-}" "$CT" php occ "$@"; }
dav() { # dav METHOD PATH [curl args...]
  local m="$1" p="$2"; shift 2
  curl -sS -u "$DAV_USER:$DAV_PW" -X "$m" "$NC$p" "$@"
}

start_container() {
  if [ -n "$(docker ps -q -f "name=^${CT}$")" ]; then
    echo "container $CT already up"
    return
  fi
  docker rm -f "$CT" >/dev/null 2>&1 || true
  docker run -d --name "$CT" -p "${NC##*:}:80" \
    -e SQLITE_DATABASE=nextcloud \
    -e NEXTCLOUD_ADMIN_USER=admin -e NEXTCLOUD_ADMIN_PASSWORD="$ADMIN_PW" \
    -e NEXTCLOUD_TRUSTED_DOMAINS="localhost 127.0.0.1" "$IMG" >/dev/null
  echo -n "installing"
  for _ in $(seq 1 90); do
    case "$(curl -s -m 5 "$NC/status.php" 2>/dev/null)" in
      *'"installed":true'*) echo " ok"; return;;
    esac
    echo -n .; sleep 5
  done
  echo " FAILED"; exit 1
}

add_user() {
  occ user:list --output=json | grep -q "\"$1\"" && return 0
  OC_PASS="$USER_PW" occ user:add --password-from-env --display-name="$2" "$1" >/dev/null
}

# A calendar object. $1 user, $2 calendar, $3 object name, $4 the component body.
put_event() {
  local u="$1" c="$2" n="$3" body="$4"
  DAV_USER="$u" DAV_PW="$USER_PW" dav PUT "/remote.php/dav/calendars/$u/$c/$n.ics" \
    -H 'Content-Type: text/calendar; charset=utf-8' \
    --data-binary "$(printf 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//practical//dev fixture//EN\r\n%b\r\nEND:VCALENDAR\r\n' "$body")" \
    -o /dev/null -w '' || true
}

vevent() { # uid summary class extra-lines
  printf 'BEGIN:VEVENT\r\nUID:%s\r\nDTSTAMP:20261001T120000Z\r\nSUMMARY:%s\r\nCLASS:%s\r\nDESCRIPTION:detail of %s\r\nLOCATION:Room %s\r\n%bEND:VEVENT' \
    "$1" "$2" "$3" "$1" "$1" "$4"
}

# Shares a collection with a user, read-write, through the sabre/dav sharing POST
# ({http://owncloud.org/ns}share — nextcloud/calendar wiki "Nextcloud specific properties").
share_to() {
  local owner="$1" path="$2" to="$3" access="${4:-read-write}"
  DAV_USER="$owner" DAV_PW="$USER_PW" dav POST "$path" -H 'Content-Type: application/xml' --data-binary \
"<?xml version=\"1.0\"?><o:share xmlns:d=\"DAV:\" xmlns:o=\"http://owncloud.org/ns\"><o:set><d:href>principal:principals/users/$to</d:href><o:summary>dev fixture</o:summary><o:$access/></o:set></o:share>" \
    -o /dev/null -w "share $access $(basename "$path") %{http_code}\n"
}

seed() {
  add_user alice Alice
  add_user bob Bob

  # --- alice's own calendars
  occ dav:create-calendar alice work >/dev/null 2>&1 || true
  occ dav:create-calendar alice mixed >/dev/null 2>&1 || true
  occ dav:create-calendar alice repeating >/dev/null 2>&1 || true
  occ dav:create-calendar alice trashed >/dev/null 2>&1 || true

  put_event alice work w1 "$(vevent w1 Standup PUBLIC 'DTSTART;TZID=Europe/Berlin:20261012T090000\r\nDTEND;TZID=Europe/Berlin:20261012T091500\r\n')"
  put_event alice work w2 "$(vevent w2 'Review' PUBLIC 'DTSTART;VALUE=DATE:20261014\r\nDTEND;VALUE=DATE:20261015\r\n')"

  for cls in PUBLIC PRIVATE CONFIDENTIAL; do
    put_event alice mixed "a-$cls" "$(vevent "a-$cls" "Alice $cls" "$cls" 'DTSTART;TZID=Europe/Berlin:20261013T100000\r\nDTEND;TZID=Europe/Berlin:20261013T110000\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:ping\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\n')"
  done

  # master + moved instance in ONE object, as a CalDAV server stores them
  put_event alice repeating r1 "$(printf '%s\r\n%s' \
    "$(vevent r1 'Weekly' PUBLIC 'DTSTART;TZID=Europe/Berlin:20261012T140000\r\nDTEND;TZID=Europe/Berlin:20261012T150000\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\n')" \
    "$(vevent r1 'Weekly (moved)' PUBLIC 'RECURRENCE-ID;TZID=Europe/Berlin:20261019T140000\r\nDTSTART;TZID=Europe/Berlin:20261019T160000\r\nDTEND;TZID=Europe/Berlin:20261019T170000\r\n')")"

  # --- a webcal subscription and a deleted calendar: both must be filtered out
  occ dav:create-subscription alice holiday-feed "https://www.officeholidays.com/ics/germany/saxony" >/dev/null 2>&1 || true
  DAV_USER=alice DAV_PW="$USER_PW" dav DELETE /remote.php/dav/calendars/alice/trashed -o /dev/null -w ''

  # --- alice's address book, with a birthday so the derived calendar appears
  occ dav:create-addressbook alice contacts >/dev/null 2>&1 || true
  DAV_USER=alice DAV_PW="$USER_PW" dav PUT /remote.php/dav/addressbooks/users/alice/contacts/c1.vcf \
    -H 'Content-Type: text/vcard; charset=utf-8' --data-binary \
    "$(printf 'BEGIN:VCARD\r\nVERSION:3.0\r\nUID:c1\r\nFN:Carla Contact\r\nN:Contact;Carla;;;\r\nBDAY:19900415\r\nEMAIL:carla@example.org\r\nEND:VCARD\r\n')" -o /dev/null -w ''
  occ dav:sync-birthday-calendar alice >/dev/null 2>&1 || true

  # --- bob shares one of each to alice
  occ dav:create-calendar bob bobs-plan >/dev/null 2>&1 || true
  occ dav:create-addressbook bob bobs-contacts >/dev/null 2>&1 || true
  for cls in PUBLIC PRIVATE CONFIDENTIAL; do
    put_event bob bobs-plan "b-$cls" "$(vevent "b-$cls" "Bob $cls" "$cls" 'DTSTART;TZID=Europe/Berlin:20261015T080000\r\nDTEND;TZID=Europe/Berlin:20261015T090000\r\n')"
  done
  share_to bob /remote.php/dav/calendars/bob/bobs-plan alice read-write
  share_to bob /remote.php/dav/addressbooks/users/bob/bobs-contacts alice read-write

  # A READ-ONLY share loses more than a read-write one, so both shapes are needed
  # (measured 2026-10-10 against this fixture, nextcloud 35.0.1):
  #   own                -> PUBLIC, PRIVATE, CONFIDENTIAL all complete
  #   shared read-write  -> PRIVATE missing; CONFIDENTIAL complete
  #   shared read-only   -> PRIVATE missing; CONFIDENTIAL reduced to DTSTART/DTEND/UID/CLASS
  #                         with SUMMARY replaced by the server's word for "Busy"; every VALARM dropped
  # Source of that split: apps/dav/lib/CalDAV/CalendarObject.php:46-62 (PRIVATE is filtered in
  # Calendar.php at four call sites, CONFIDENTIAL only when `isPublic() || !canWrite()`).
  occ dav:create-calendar bob bobs-readonly >/dev/null 2>&1 || true
  for cls in PUBLIC PRIVATE CONFIDENTIAL; do
    put_event bob bobs-readonly "ro-$cls" "$(vevent "ro-$cls" "RO $cls" "$cls" 'DTSTART;TZID=Europe/Berlin:20261016T080000\r\nDTEND;TZID=Europe/Berlin:20261016T090000\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:ping\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\n')"
  done
  share_to bob /remote.php/dav/calendars/bob/bobs-readonly alice read
}

case "${1:-up}" in
  up)
    start_container
    seed
    echo
    echo "Nextcloud $NC   alice / $USER_PW   admin / $ADMIN_PW"
    occ dav:list-calendars alice || true
    ;;
  down) docker rm -f "$CT" >/dev/null 2>&1; echo "removed $CT";;
  reseed) seed;;
  *) echo "usage: $0 [up|down|reseed]"; exit 2;;
esac
