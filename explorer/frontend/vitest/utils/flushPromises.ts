const scheduler = setTimeout;

export default function flushPromises() {
  return new Promise(function(resolve) {
    scheduler(resolve);
  });
}
