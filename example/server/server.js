const express = require('express');
const cookieParser = require('cookie-parser');
const session = require('express-session');

const app = express();

app.use(cookieParser());
app.use(session({
  secret: "driiilll!",
  resave: false,
  saveUninitialized: false
}));

const handler = function(req, res){
  res.json({ status: ':D' })
};

// Standard test plan
app.get('/api/organizations', handler);
app.get('/api/users.json', handler);
app.get('/api/users/contacts/:id', handler);
app.get('/api/subcomments.json', handler);
app.get('/api/comments.json', handler);
app.get('/api/users/:id', handler);
app.post('/api/users', handler);
app.get('/api/account', handler);

// Sessions test plan
app.get('/login', function(req, res){
  if(req.query.user === 'example' && req.query.password === '3x4mpl3'){
    req.session.counter = 1;
    res.send("Welcome!");
  } else {
    res.status(403).send('Forbidden');
  }
});

app.get('/counter', function(req, res){
  if(req.session.counter){
    req.session.counter++;
    res.json({counter: req.session.counter})
  } else {
    res.status(403).send('Forbidden');
  }
});

app.get('/', function(req, res){
  res.json({ status: ':D' })
});

app.delete('/', function(req, res){
  req.session.counter = 1;
  res.json({counter: req.session.counter})
});

app.listen(9000);
