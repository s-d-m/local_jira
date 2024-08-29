# The story behind this Local_Jira

Local_jira started as a personal project to scratch an itch I had. Loading jira tickets using
the web UI was taking some time and I decided to see if it could be improved.

Before starting this project, I looked to see if something already existed that I could
use instead. Unfortunately the solutions I found ([jiracli](https://github.com/ankitpokhrel/jira-cli) 
and [jirust](https://github.com/Code-Militia/jirust)) were unsatisfying as they were still too slow
to work with. Turns out, issuing even a single request using curl or wget to fetch tickets
using the Jira API takes seconds. Therefore, any tool which depend on the jira server (such as these two)
can't be fast.
Another solution in this space would be [jira client](https://almworks.com/jiraclient/overview.html)
but unfortunately the product is discontinued and I couldn't find out where to download the
last version.

Consequently, I started my own jira client that would fix the slow loading times by using
a simple trick: caching. Since most data on tickets rarely change, this is a perfect case
for caching. `Local_jira` at its core, uses the `jira` API from Atlassian to download a copy
of the tickets, and save them in a local database. Later, a user can query the local database
and open a ticket in milliseconds instead of seconds.


# Goals (aka high level requirements)

## Fast interaction

The main goal of the project is to allow a user to navigate jira tickets without noticeable
delay. Clicking on a ticket should instantly load the relevant data and display them.

## Easy to use

A user shouldn't need to write SQL code and learn the database schema to extract the requested
data. In fact, this would defeat the purpose of the project since it would then become
faster to wait on the jira server.

Instead, the user should be able to interact with a nice interface that should be as intuitive
as possible.  A user shouldn't need a PhD in ticket tracking software to use this software.
In fact, it should  be so simple to use, there shouldn't even be a need for a tutorial-style
documentation.

Note that an advanced user should still be allowed to write sql queries if it wants to. The
point is that it mustn't be required to do so for basic usage.

## Allow to modify tickets in a simple and quick way

Initially, I was picturing a UI similar to jira, where a user could easily edit field texts,
add or remove labels, change a ticket status ... by simply clicking on fields and interacting
with them. This is of course possible, but comes with a complication common to distributed
system or software that can be used offline. How should the software behave when the user
goes offline, edit a field locally, then goes online, and that field has been edited by
someone else in the meantime?

While there are answers to these questions, I didn't go far enough in the project to provide
the edit capabilities. The point of this project was to alleviate my pain at work using jira
and I realised that I wasn't editing tickets often enough to use my free time implementing
these features. The project is still designed to allow for implementing such features in the
future shall someone decide to work on it. In the mean time, for editing, I could fall back
to using [jira-cli](https://github.com/ankitpokhrel/jira-cli) instead when possible, or endure the
slowness of real jira otherwise.

## Preemptively synchronise the local database

The design uses a cache to accelerate interactions, but with caching comes caching issues,
namely stale data. To avoid this, the software should periodically interrogate the jira server
for updates, and fetch those data. Ideally, the software would be notified by the jira server
to ensure that data is always up-to-date but this seemed out of reach, or at least required
more work than I was willing to put in.

On top of periodically checking for updates, it should be possible to manually trigger a
synchronisation as to avoid the need to wait to ensure fresh data.

The point of preemptively updating the database is to reduce the time windows for stale data.

## Get notified on retrieved ticket changes

Users shouldn't have to worry about out-of-date data. Unfortunately this is at odds with fast
interactions. There is always a time window where the local database will be out-of-sync
between two synchronisation points, when a change is registered on the remote.
As a compromise between fast-interaction, and latest data, the software should:
1. display the local data immediately
2. synchronise with the remote server
3. detect if there was a change
4. in case there was a change, notify the user so he can refresh the screen with the latest data

## Download as little from jira as required

When a change is detected on a single ticket, the software should fetch as little data as
necessary from the remote server to incrementally update the local database to bring it
to a up-to-date state. This is necessary to minimise the time between the first and last
point on notifying users of changes.


# Non-goal, explicitly out-of-scope features

## Only support features I use
Jira is a tool with many features. Many of which are features that I either don't use, or use so
rarely that the time spent in developing features in `Local_jira` for them would be more trouble
than accepting to use the real jira server.

Such features include:
- history (the ability to see former versions of a ticket, what changed, and who edited it)
- status boards (the dashboard grouping tickets with the same status into columns, where changing a ticket's status can be done by drag and drop between columns )
- personal dashboard
- saved jql queries

There are likely many more features I don't even know about. These non-goals
manifest themselves in that the local database schema is significantly simpler
than the one from jira ([database schema available here](https://developer.atlassian.com/server/jira/platform/database-schema/))

## No support for data replication

Ensuring that data is always available even after a computer catches fire is
nice, but takes work to achieve. In my case, if the computer gets damaged
beyond repair, all it takes is to re-download everything from scratch, re-going
through the first-time setup. Hopefully jira server's themselves are redundant.
