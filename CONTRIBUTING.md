a nice feature would be running `exec` for every request.
I have tried this using the existing `exec` how ever if we run a `request` with-items (csv) the assign will store the last response only.

So I have implemented an `exec` within the request, due to the modular object oriented logic this feature was pretty easy (even though this is the first time I write in rust)

I'm willing to share the change with you and some example file
