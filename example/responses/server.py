import SimpleHTTPServer
import SocketServer
import time
import random

PORT = 9000

class DelayedHandler(SimpleHTTPServer.SimpleHTTPRequestHandler):
    def do_GET(self):
        time.sleep(random.random())
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{}')


httpd = SocketServer.TCPServer(("", PORT), DelayedHandler)

print "serving at port", PORT

httpd.serve_forever()
