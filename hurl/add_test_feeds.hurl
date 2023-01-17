# First, login
POST http://localhost:4052/login
[FormParams]
email: vincent@rischmann.fr
password: foobar

HTTP/1.1 303
[Captures]
session_id: cookie "session_id"
[Asserts]
cookie "session_id" exists

# Now add the test feeds

# Tailscale
POST http://localhost:4052/feeds/add
[Cookies]
session_id: {{session_id}}
[FormParams]
url: https://tailscale.com

# Tailscale
POST http://localhost:4052/feeds/add
[Cookies]
session_id: {{session_id}}
[FormParams]
url: https://lemire.me/blog/

# Brendan Gregg
POST http://localhost:4052/feeds/add
[Cookies]
session_id: {{session_id}}
[FormParams]
url: https://www.brendangregg.com/blog/rss.xml

# LWN.net
POST http://localhost:4052/feeds/add
[Cookies]
session_id: {{session_id}}
[FormParams]
url: https://lwn.net
