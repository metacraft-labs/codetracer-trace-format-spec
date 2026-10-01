function createUser(name, age) {
  return { name: name, age: age, tags: ["user", "active"] };
}

function processUsers(users) {
  var names = [];
  for (var i = 0; i < users.length; i++) {
    names.push(users[i].name);
  }
  return names;
}

function makeCounter() {
  var count = 0;
  return function increment() {
    count = count + 1;
    return count;
  };
}

var alice = createUser("Alice", 30);
var bob = createUser("Bob", 25);
var names = processUsers([alice, bob]);
var counter = makeCounter();
var c1 = counter();
var c2 = counter();
console.log("names:", names);
console.log("counter:", c2);
