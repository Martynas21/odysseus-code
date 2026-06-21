---
name: setup-searxng
description: Stand up a local SearXNG instance with JSON search and wire it into the agent.
steps:
  - Check that Docker is installed and the daemon is running
  - Write a SearXNG settings.yml that enables the JSON API
  - Start the SearXNG container and wait for it to respond
  - Verify a JSON search query returns results
  - Set the agent's searxng_url config and tell the user to restart
---

Follow these steps to set up SearXNG so the agent can search the web. SearXNG
runs as a Docker container exposing a JSON search API on port 8080. If any
command fails, stop and report the exact error to the user instead of
continuing.

1. **Check prerequisites.** Run `docker --version` and `docker info` with the
   shell tool. If Docker is not installed or the daemon is not running, stop and
   tell the user to install Docker / start the Docker daemon, then re-run this
   skill.

2. **Write the settings file.** Create the directory `$HOME/.config/searxng` and
   write `$HOME/.config/searxng/settings.yml` with the content below. Generate a
   random secret and substitute it for `REPLACE_WITH_RANDOM_SECRET` (e.g.
   `openssl rand -hex 32`). `formats` must include `json` or the API returns an
   error, and `limiter: false` keeps automated localhost queries from being
   blocked:

       use_default_settings: true
       server:
         secret_key: "REPLACE_WITH_RANDOM_SECRET"
         limiter: false
         bind_address: "0.0.0.0"
         port: 8080
       search:
         formats:
           - html
           - json

3. **Start the container.** First check that port 8080 is free (e.g.
   `ss -ltn | grep :8080`); if it is taken, pick another port and use it
   consistently below. Then remove any pre-existing container with the same name
   so re-runs succeed, and start a fresh one:

       docker rm -f searxng 2>/dev/null || true
       docker run -d --name searxng -p 8080:8080 \
         -v $HOME/.config/searxng:/etc/searxng searxng/searxng

   Wait a few seconds, then poll `curl -s -o /dev/null -w '%{http_code}'
   http://localhost:8080/` until it returns `200`. If the container exits, read
   its logs with `docker logs searxng` and report the error.

4. **Verify JSON search.** Run
   `curl -s 'http://localhost:8080/search?q=hello&format=json'` and confirm the
   response is JSON containing a non-empty `results` array. If it is HTML or an
   error, the `formats` setting did not take effect — recheck step 2 and restart
   the container with `docker restart searxng`.

5. **Wire it into the agent.** Set the agent config value `searxng_url` to
   `http://localhost:8080` (use whatever port you settled on). Tell the user
   that the `web_search` tool reads this value at startup, so they must restart
   odysseus-code for web search to become available.

As you finish each step, call complete_skill_step so your progress is tracked.
