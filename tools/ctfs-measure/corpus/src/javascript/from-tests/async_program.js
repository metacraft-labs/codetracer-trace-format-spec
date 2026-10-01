async function fetchData(id) {
  await new Promise(r => setTimeout(r, 10));
  return "data-" + id;
}

async function main() {
  const results = await Promise.all([fetchData(1), fetchData(2)]);
  console.log(results.join(", "));
}

main();
