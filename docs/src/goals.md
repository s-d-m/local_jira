# The story behind this Local_Jira

Local_jira started as a personal project to alleviate some pain I had at my day job.
For reasons I won't explain here, the project setup at the workplace was such that each engineers
had to work with dozens of jira tickets simultaneously. I don't  think anyone got to 100 simultaneous
tickets but for over 50, I'm sure.

Unfortunately, opening one single jira ticket in a web browser is excruciatingly slow. Loading
a small ticket with only 5 comments on it and 5 links to other tickets takes about 9 seconds.
Multiply that by the dozens of tickets, switching back and forth in the browser tabs and 
you can understand why this became a bottleneck.

Before starting this project, I looked to see if something similar already existed that I could
use instead. Unfortunately the solutions I found ([jiracli](https://github.com/ankitpokhrel/jira-cli) 
and [jirust](https://github.com/Code-Militia/jirust)) were unsatisfying as they were still too slow
to work with. Turns out, issuing even a single request using curl or wget to fetch tickets
using the Jira API takes seconds.
Another solution in this space would be [jira client](https://almworks.com/jiraclient/overview.html)
but unfortunately the product is discontinued and I couldn't find out where to download the
last version.

Consequently, I started my own jira client that would fix the slow loading times by using
a simple trick: caching. Since most data on tickets rarely change, this is a perfect case
for caching. `Local_jira` at its core, uses the `jira` API from Atlassian to download a copy
of the tickets, and save  it in a local database. Later, a user can query the local database
and open a ticket in milliseconds instead of seconds.

# Goals (aka high level requirement)

## Fast interaction

The main goal of the project is to allow a user to navigate jira tickets without noticeable
delay. Clicking on a ticket should instantly load the relevant data and display them. In order
to achieve that goal, no network request shall appear on the critical path between the user
click and the screen update.

The trick as already explained above is to save data locally.
Nowadays, disk access are fast enough that using an SQL database for storage without further
optimisation is good enough.

## Easy to use

A user shouldn't need to write SQL code and learn the database schema to extract the requested
data. In fact, this would defeat the purpose as the project since it would then become
faster to wait on jira slowness.
Instead, the user should interact with a nice interface that should be as intuitive as possible.
A user shouldn't need a PhD in ticket tracking software to use this software. In fact, it should
be so simple to use, there shouldn't even be a need for a tutorial-style documentation.

## Preemptively synchronise the local database

The design uses a cache to accelerate interactions, but with caching comes caching issues,
namely stale data. To avoid this, the software should periodically interrogate the jira server
for updates, and fetch those data. Ideally, the software would be notified by the jira server
to ensure that data is always up-to-date but this seemed out of reach, or at least required
more work than I was willing to put.

On top of periodically checking for updates, it should be possible to manually trigger a
synchronisation as to avoid the need to wait to ensure fresh data.

The idea of preemptively updating the database is to reduce the time windows for stale data.

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
to a up-to-date state.
