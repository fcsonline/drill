const fs = require('fs');
const path = require('path');
const express = require('express');
const cookieParser = require('cookie-parser');
const session = require('express-session');

const app = express();
const delay = process.env.DELAY_MS || 0;

app.use(cookieParser());
app.use(session({
  secret: "driiilll!",
  resave: false,
  saveUninitialized: false
}));


const logger_handler = (character) => {
  return (req, res) => {
    setTimeout(() => {
      const filename = path.join(__dirname, 'responses', req.path)
      fs.readFile(filename, 'utf8', function(err, data) {
        process.stdout.write(character);

        if (err) {
          res.status(404);
          res.end();
        } else {
          res.write(data);
          res.end();
        }
      });
    }, delay);
  };
}

const randomFailedHandler = function(req, res) {
  const number = Math.round(Math.random() * 50);

  if (number === 20) {
    res.status(500).json({ status: ':/' });
  } else {
    res.json({ status: ':D' });
  }
};

// Standard test plan
app.get('/api/organizations', logger_handler('O'));
app.get('/api/users.json',  logger_handler('U'));
app.get('/api/users/contacts/:id', logger_handler('T'));
app.get('/api/subcomments.json', logger_handler('S'));
app.get('/api/comments.json', logger_handler('C'));
app.get('/api/users/:id', logger_handler('I'));
app.post('/api/users', randomFailedHandler);
app.get('/api/account', logger_handler('A'));

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
console.log('Listening on port 9000...');
