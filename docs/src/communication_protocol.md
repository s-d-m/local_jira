# Communication protocol

The communication protocol is a message protocol "designed" to be very simple to parse. If I had
infinite time and motivation, I'd have probably use something ready-made like gRPC. Or something
based on AVRO. At the very least, it would be a binary encoded protocol.

Instead, I went for something simplistic to save time and move on with other parts of the project.
The communication protocol can easily be swapped out for something better though.

Messages are sent and received through the stdin/stdout of the server and can be separated in two
categories:
1. request messages
2. reply messages

Messages are written using ascii-only strings, and separated by newlines. In other words,
waiting and reading a message is as simple as calling the C++ `std::getline` function.
This is obviously a poor communication protocol since there is a very easy way to cause
a denial of service by forcing the server to allocate more and more memory. Simply keep
writing bytes in its stdin, but never write a newline character.

## Request format

One request can trigger one or more replies.

Each request has a simple format:
```
<token><space><request>[<space><request,parameters,separated,by,commas>]<newline>
```

Valid requests always contain two or three chunks separated by a space character.
Some request takes parameters and some don't.

For the case of request taking no parameters, the following form shall be used:
`<token><space><request><newline>`. It is to be noted that passing an empty parameter to a
request taking no parameter must be rejected by the server. In other words, using
`<token><space><request><space><newline>` is an error and no request will be processed.

For the case of requests taking parameters, the following form is to be used:
`<token><space><request><space><request,parameters,separated,by,commas><newline>`
If a request has more than one parameter, those must be concatenated together, separated by
comma characters. Please note that having two consecutive commas means passing a
parameter which is empty. Similarly, if the list of parameters is terminated by a comma,
the server will interpret it as having an empty parameter after that comma. To put
it simple, commas serve the role of separators, with no exceptions of particular case.


### token

The token is a client generated unique id to identify the conversation, i.e. associate all
replies with the request that triggered them. It is the responsibility of the client to
ensure the tokens are unique and never reused. The server will simply return that token along
with the replies.

A token can only contain the following characters: `[a-zA-Z-0-9]` and `-`. For example,
`DL-TICKET-PROJ-124-ffhxe` is a valid token. `asdf_df12` isn't, because of the underscore

Each request must provide a unique token. One way to ensure they are unique is by appending an
increasing number.

### request

The request can be one of the following:
- `FETCH_TICKET`: used to fetch data for a specific ticket
- `FETCH_TICKET_LIST`: used to fetch a list of jira issue keys
- `FETCH_TICKET_KEY_VALUE_FIELDS`: used to fetch the key value fields of a specific ticket
- `FETCH_ATTACHMENT_LIST_FOR_TICKET`: used to retrieve the list of attachment belonging to a ticket
- `FETCH_ATTACHMENT_CONTENT`: used to retrieve an attachment
- `SYNCHRONISE_TICKET`: use to synchronise a specific ticket
- `SYNCHRONISE_UPDATED`: used to synchronise the tickets that were added or updated since last synchronisation point.
- `SYNCHRONISE_ALL`: used to trigger a full database resynchronisation
- `EXIT_SERVER_AFTER_REQUESTS`: used to tell the server to stop accepting requests and exit after finishing processing the current on-going ones.
- `EXIT_SERVER_NOW`: used to tell the server tp stop processing any on-going request, not accept any new ones, and exit immediately.

### request parameters

Some requests can have parameters to pass to the server. Such parameters are passed
encoded as string separated by commas. Parameters themselves can only contain ascii
characters in the following character set: `[a-zA-Z-0-9]` and `-`.

Note that a space character must always appear immediately after the request, even when
there are no extra parameters.

*FETCH_TICKET*: used to fetch data of a specific ticket.
This command takes two parameters. The first one is the ticket's key to fetch (e.g. `PROJ-123`).
The second one is the requested format of the reply. Can be one of `MARKDOWN`, or `HTML`.

*FETCH_TICKET_LIST*: used to retrieve all available ticket's key in the local database.
Takes no parameter.

*FETCH_TICKET_KEY_VALUE_FIELDS*: used to fetch the key value fields of a specific ticket.
This command takes one parameter: the ticket's key (e.g. `PROJ-123`).

*FETCH_ATTACHMENT_LIST_FOR_TICKET*: used to retrieve the list of attachment belonging to a ticket.
Takes one parameter: the ticket's key for which to retrieve the attachment (e.g. PROJ-231)

*FETCH_ATTACHMENT_CONTENT*: used to retrieve the content of an attachment.
Takes one parameter: the uuid of the attachment to fetch.

*SYNCHRONISE_TICKET*: used to synchronise a ticket with the jira remote, thus ensuring getting
up-to-date data in teh local database. Takes one parameter, the ticket's key to synchronise
(e.g. `PROJ-456`)

*SYNCHRONISE_UPDATED*: used to synchronise the tickets that were added or updated since last
synchronisation point. Doesn't take parameters

*SYNCHRONISE_ALL*: used to resynchronised the projects. This basically request to update all tickets
that were modified on the server since the last synchronisation point. Doesn't take any parameter.

*EXIT_SERVER_AFTER_REQUESTS*: takes no parameter.

*EXIT_SERVER_NOW*: takes no parameter.

## Reply format

In order for the client to know which reply corresponds to which request, the server returns the
request id the client passed to it initially. In case of server initiated message, the id is the
special value marked with a single underscore.

A reply has the following format:

```
<request id><space><STATUS KEYWORD>[<space><data encoded in string>]<newline>
```

It is to be noted that a server can return more than one reply to a single request. This is
for example the case when the server immediately returns local data, only to realise those were
out of date and new data is available.

## requests acknowledgment and liveness

Immediately upon receiving a well-formed request, the server will acknowledge it by immediately
returning the following message:
```
<request id><space>ACK<newline>
```

On malformed request, it will immediately return the following:
```
_<space>ERROR[<space><an error message>]<newline>
```

From the moment the server returns the message `<request id><space>ACK<newline>` the request
is in an ongoing state. The server can keep returning replies to that request.

The server will notify the client that the request is finished by issuing the message
```
<request id><space>FINISHED<newline>
```

### replies in case of errors

To notify the client of an error, the server will reply with a message starting by the request id (or underscore),
followed by a space, followed by the keyword ERROR, followed by another space. The rest of the message
is an error message. Note, there will always be a space after the Error keyword, even if the error message
is empty.

For example:
```
<request id><space>ERROR<space>unknown request<newline>
<request id><space>ERROR<space>invalid parameter for request<newline>
_<space>ERROR<space>invalid request<newline>
<request id><space>ERROR<space>Error occurred. Couldn't connect to jira server<newline>
```

It is to be noted that an error doesn't automatically terminate the request. The server will
still issue a finished message for a request after an error. Only in the special case of a
malformed request will there be no finished message. But in the case of a malformed request
there will be no ack message either to begin with.


### replies generated by a FETCH_TICKET query

Upon receiving a valid FETCH_TICKET query, the server will reply with
```
<request id><space>RESULT<space><base64 encoded data><newline>
```

The base64 encoded data is the answer in either markdown or html format, depending on what
was requested. To put it simple, this is what should be displayed on the screen after decoding.

A server can implement this request by:
1. immediately returning data from the local database
2. synchronising in the background the requested ticket
3. returning the newest data using the same format message
4. terminating the transaction with the FINISHED message

Another implementation can be to do the step 3 above only if there were data changes.
Yet another one could also not synchronise and immediately terminate the query after step 1.

A server can also return error messages specific to that request during the processing.
A client can decide to display these error messages to a user or swallow them.


### replies generated by a FETCH_TICKET_LIST query

Upon receiving a valid FETCH_TICKET_LIST query, the server will reply (in case of success) with
```
<request id><space>RESULT<space><jira issue keys all separated by commas><newline>
```

Hopefully, jira issue keys will always have the form <uppercase letter><dash><numbers> and therefore
this reply won't break anything.

similarly to a FETCH_TICKET query, a server implementation can decide to terminate the query
after sending the data immediately available in the database or instead keep the query on going,
resynchronise everything and then issue the newest data.


### replies generated by a FETCH_TICKET_KEY_VALUE_FIELDS query

Upon receiving a valid FETCH_TICKET_KEY_VALUE_FIELDS query, the server will reply (in case of success) with
```
<request id><space>RESULT<space><list of key value pair><newline>
```

The encoding of the list of the key value pair is as follow:
each key and value are encoded in base64 and passed as `<key in base64><colon character><value in base64>`
key value pairs are separated by commas.

For example, the following string
```
ZGVzY3JpcHRpb24K:ZHVtbXkgdGlja2V0IHRvIHRlc3Qgc29tZXRoaW5nIG9uIGppcmEgc2VydmVyCg==,c3RhdHVzCg==:RG9uZQo=
```
shall be decoded to:
```
key=description
value=dummy ticket to test something on jira server

key=status
value=done
```

It is to be noted that the order of the keys are not guaranteed.
The string `c3RhdHVzCg==:RG9uZQo=,ZGVzY3JpcHRpb24K:ZHVtbXkgdGlja2V0IHRvIHRlc3Qgc29tZXRoaW5nIG9uIGppcmEgc2VydmVyCg==`
is semantically equivalent to the above one. If the client needs to rely on a specific ordering, it is its
responsibility to sort the key/values accordingly.

similarly to a FETCH_TICKET query, a server implementation can decide to terminate the query
after sending the data immediately available in the database or instead keep the query on going,
resynchronise everything and then issue the newest data.



### replies generated by a FETCH_ATTACHMENT_LIST_FOR_TICKET query

Upon receiving a valid FETCH_ATTACHMENT_LIST_FOR_TICKET query, the server will reply (in case of success) with
```
<request id><space>RESULT<space><list of uuid filename pair separated by commas><newline>
```

the list of uuid filename is provided using the following schema:
```
<uuid><comma character><filename base64 encoded>
```
Pairs of uuid,filename are separated by commas.
For example, for a ticket containing two attachments "specification.pdf" and "addendum.png", a reply
can be:
```
get_attchlist-lkt54er RESULT b23f95f0-db34-4739-9fde-2c0a107d9c97:c3BlY2lmaWNhdGlvbi5wZGYK,b7680c80-a28c-4f09-9ee8-272ec6682ae5:YWRkZW5kdW0ucG5nCg==<newline>
```

Similarly to replies of a FETCH_TICKET_KEY_VALUE_FIELDS request, the order of the filenames is not guaranteed.
The above example is semantically equivalent to the following one:
```
get_attchlist-lkt54er RESULT b7680c80-a28c-4f09-9ee8-272ec6682ae5:YWRkZW5kdW0ucG5nCg==,b23f95f0-db34-4739-9fde-2c0a107d9c97:c3BlY2lmaWNhdGlvbi5wZGYK<newline>`
```

In case  of a ticket containing no attachment, the returned list will be empty. However, there
must still be a space character between RESULT and the newline.

Note: similarly to a FETCH_TICKET query, a server implementation can decide to terminate the query
after sending the data immediately available in the database or instead keep the query on going,
resynchronise everything and then issue the newest data.


### Replies generated by a FETCH_ATTACHMENT_CONTENT ticket.

Upon receiving a valid FETCH_ATTACHMENT_LIST_FOR_TICKET query, the server will reply (in case of success) with
```
<request id><space>RESULT<space><base64 encoded file content><newline>
```

The server can obviously reply with errors, for example:
```
<request id><space>ERROR<space>no such uuid in local database<newline>
```

As per any request, it is finished by a FINISHED reply.

### Replies generated by a SYNCHRONISE_TICKET request

When receiving a SYNCHRONISE_TICKET request, the server will notify the start of the synchronisation
by issue a reply with the following format:
```
<request id><space>RESULT<space>synchronisation started<newline>
```
This message will appear after the ACK. It will be followed by another message saying
```
<request id><space>RESULT<space>synchronisation finished<newline>
```
in the success case.

Of course, errors can happen, for example if the given ticket doesn't exists on the remote
or the user doesn't have access to it. In such case, the server will return an error.

Like any other request, the server will terminate the processing with a FINISHED message.


### Replies generated by a SYNCHRONISE_UPDATED

The replies generated by a SYNCHRONISE_UPDATED request are the same as the ones generated by
a SYNCHRONISE_TICKET request.

### Replies generated by a SYNCHRONISE_ALL

The replies generated by a SYNCHRONISE_ALL request are the same as the ones generated by
a SYNCHRONISE_TICKET request.


### Replies generated by a EXIT_SERVER_AFTER_REQUESTS request

When receiving a EXIT_SERVER_AFTER_REQUESTS, the server will reply with the ACK like for
any other message. The server won't process nor event acknowledge any new request anymore
except a EXIT_SERVER_NOW. When the last currently on-going request will finish, the server
will return a FINISHED message, and exit immediately after.

When the server is requested to exit after on-going requests are finished, it can still
accept a EXIT_SERVER_NOW request. This way, it is possible for a client to gracefully
exit the server, giving it some time for cleanup, and if it is still running after some time,
tell it to quit immediately without finishing any background task.

### Replies generated by a EXIT_SERVER_NOW

When receiving a EXIT_SERVER_NOW, the server will reply with the ACK like for
any other message. The server won't acknowledge any new request anymore, instead
the server will immediately quit (or try to).


# Notes on the server lifecycle

With interactive software doing background work, it is unfortunately easy to accidentally
introduce a bug during development, that makes the server wait forever instead of shutting
down. To help signalling the client about the server's state, the server will try to return
a FINISHED message right before quitting.

It is possible (and likely) that the server quits before this message is sent, therefore
a client shouldn't assume that the server is running until this message is sent.
Instead, a client should use the system's job processing tools (e.g. `ps`) to see if the
server is still running.

If a client receives a FINISHED message for a EXIT_SERVER_AFTER_REQUESTS or a
EXIT_SERVER_NOW, the client can assume that the server finished all the processing he
was doing, and will immediately quit after. If for some reason the server is still running
after sending a FINISHED message for a EXIT_SERVER_AFTER_REQUESTS or EXIT_SERVER_NOW, the
server can be safely killed using `kill -9` without worrying about resource cleanups.
It is the server's responsibility to ensure that all persistent resources used are
cleaned up before it issues a FINISHED message.
