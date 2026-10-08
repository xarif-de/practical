# Features

Everything RustiCal stores, made usable in the browser — in German, English, French and Spanish, light and dark,
desktop and phone.

## Calendar

- Views: day, week, month, year planner; under "More": free weekends, vacation planner (counts vacation days
  against an entitlement, without weekends and public holidays), countdown wall, year in numbers.
- Trackpad and touch: one month or year per swipe in month/year view; a sideways swipe switches between views.
- Public and school holidays per region ([configuration.md](configuration.md#holidays)), shown in every view and
  counted by the vacation planner.
- Live: a change from a phone or desktop app appears in open tabs within seconds.

## Event and task editor

- Opens in a read view; the lock switches to editing. Cards only where there is data: when, where (address search,
  map), people, reminders, recurrence (with a preview strip), links and attachments, video-call links, status,
  notes with `@contact`, `#category` and `!event` mentions.
- Keeps every iCalendar property it does not model: nothing a calendar app wrote gets lost.
- **Templates per category** ("rules"): a category can pre-fill or lock fields — colour, all-day, reminders,
  location, priority, … — for events and tasks. **Title templates**: typing the title of an earlier event offers
  it as a template.

## Tasks

- Views: groups (overdue, today, this week, later, …), table, board, timeline, cards, history of done tasks; one
  sort and time range for all of them.
- Priorities, progress, start and due dates, status, categories, location, links.

## Contacts

- List with selectable columns, sorting and address-book colours; birthdays on the countdown wall.
- Editor with view and edit mode: phones, mails, addresses, dates, organisation, messengers, relations, notes;
  vCard properties it does not model are kept.

## Search

- One search field for events, contacts and tasks, with dates, prefixes (`event:`, `contact:`, `task:`) and a
  search history; a full result dialog with up to 200 hits per kind.

## Colours

- Every colour in one place: calendars, categories, events, tasks, holidays, address books.
- **Colour optimiser**: six levels from "Gentle" (same title → same colour) to "New palette"; unifies what belongs
  together, pulls apart colours that are hard to tell apart, and lists every change with its reason before it is
  applied — drag an element to another action, or leave it out. Calendars, holidays and colours a user picked on
  purpose stay as they are.

## Change history

- Every change — from the browser or from any DAV client — with the device that made it, what changed, and undo
  (also of deletes, also after RustiCal purged the object).

## Administration (in the settings)

- Groups owned together by their members: shared calendars and address books per group.
- Calendars and address books per person or group; `.ics`/`.vcf` import and export.
- App tokens per device for calendar apps (DAVx⁵, Thunderbird, Apple Calendar, …), shown once.
- A trash with a grace period for deleted groups and calendars.
- Browser push reminders, per device.
