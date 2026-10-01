function delay(ms) {
  return new Promise(function(resolve) {
    setTimeout(resolve, ms);
  });
}

async function step1() {
  await delay(5);
  return "step1-done";
}

async function step2(prev) {
  await delay(5);
  return prev + " -> step2-done";
}

async function main() {
  var r1 = await step1();
  var r2 = await step2(r1);
  console.log(r2);
}

main();
