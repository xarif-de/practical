// Stops exactly the servers this run started (<run dir>/pids of every slot, lib/servers.mjs): SIGINT, then SIGKILL
// after 5 s. Workers stop their own pairs; what remains here is a crashed worker's or the shared pair.
import { runDirs } from './global-setup.mjs';
import { stopPids } from './lib/servers.mjs';

export default async function globalTeardown(config) {
  for (const d of runDirs(config)) await stopPids(d);
}
