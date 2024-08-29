# Notes

## Protocol communication

I don't like this communication protocol, but didn't want to spend time comparing different
rpc mechanisms. This here is "designed" to be easily implementable using
1. getline()
2. split(' ') # split string on spaces
3. split(',') # split string on commas
4. base64 encode/decode

with no need to introduce complex dependencies.

This protocol incurs significant overhead due to base64 encoding/decoding and string processing
instead of just passing raw data.


## Design "issue"

Initially I had in mind that the client shouldn't need to know about the database and its schema.

The idea was that if a feature is useful for a client, it should be provided by the server, such
that reimplementing the client with a different language or framework would be easy and only focus
on the UI part. One example here: the code that transforms the Atlassian Document Format into human
readable text or into HTML isn't client-specific and is therefore in the server.

On the plus part, and this was my initial motivation behing that choice, if the client doesn't know
about the local database, he can't concurrently access it, especially with writes and accidentally
corrupt it. Changing the database schema also becomes simple since only the server is impacted and
needs to be updated, not any of the clients.

This can be seen as a design choice, but comes with other problems. When displaying a ticket,
a client might want to show the status on the top right, next to the last modification.
Another client might prefer to set a background colour that corresponds to the ticket status
instead.

Allowing for each of these possible customisation means restricting the possible views a client can
display to a few approved ones hardcoded on the server. Every change a client might want here would
need support on the server. This doesn't scale with client customisation choices.

One way to satisfy this, while still hiding the database to the clients would be to provide
a kind of scripting language or template language that the server would interpret in order
to generate a view a client wants. A client would pass a script it controls to the server
which when interpreted would generate a view to be displayed.

This is too much work in comparison to the alternative of simply letting the client access
the database and retrieve and format all information the way it wants. I now consider that
letting the clients directly access the database is less trouble for me. It is certainly less
elegant than introducing a scripting or templating language, but also significantly less
maintenance work.
Plus, it would also mean less encoding/decoding when passing data from the server to the client.