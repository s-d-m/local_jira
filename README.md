LOCAL_JIRA
=====

Local_jira is a project I started to scratch an itch I had.

At work we use [jira](https://www.atlassian.com/software/jira) as an issue tracker.

Unfortunately, the setup there is such that is takes about 8-10 seconds to display a ticket, making for
a subpar user experience. Since tickets are seldomly edited, there is a great opportunity for caching.

`Local_jira` at its core simply replicates the list of tickets in a local database, and synchronises it from
time to time. Users can then later fetch data from the local database in milliseconds instead of seconds.

`Local_jira` is developed following a client/server model, such that the GUI is completely separated and can
therefore be independently changed.

A GUI is provided in the [jira_gui](https://codeberg.org/s-d-m/jira_gui) repository and is probably more of
interest for a user.

The documentation for `Local_jira` itself is available [here](https://s-d-m.codeberg.page/local_jira/)


