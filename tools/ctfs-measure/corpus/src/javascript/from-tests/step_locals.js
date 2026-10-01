function compute(a, b) {
  const base = a + b;        // const declaration
  let scaled = base * 2;     // let declaration
  var offset = 10;           // var declaration
  scaled = scaled + offset;  // reassignment
  return scaled;
}
compute(10, 32);
