const express = require('express');
const cookieParser = require('cookie-parser');
const session = require('express-session');

const app = express();

app.use(cookieParser());
app.use(session({secret: "Shh, its a secret!"}));

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

app.listen(3000);
