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

2. **Write the settings file.** Run the two commands below on the **host shell**
   (the same shell as the other steps — do NOT write this file from inside a
   Docker container, or it will be owned by the container's user and you will not
   be able to edit it). Generating the secret with command substitution into a
   single `printf` guarantees it stays on one line — never hand-write or paste the
   secret across lines, which produces invalid YAML and crashes SearXNG on boot.

       mkdir -p "$HOME/.config/searxng"
       printf 'use_default_settings: true\nserver:\n  secret_key: "%s"\n  limiter: false\n  bind_address: "0.0.0.0"\n  port: 8080\nsearch:\n  formats:\n    - html\n    - json\n' "$(openssl rand -hex 32)" > "$HOME/.config/searxng/settings.yml"

   This produces the following file. `formats` must include `json` or the API
   returns an error, and `limiter: false` keeps automated localhost queries from
   being blocked:

       use_default_settings: true
       server:
         secret_key: "<64-hex-char secret on one line>"
         limiter: false
         bind_address: "0.0.0.0"
         port: 8080
       search:
         formats:
           - html
           - json

   Confirm the file is valid before continuing: `cat "$HOME/.config/searxng/settings.yml"`
   — the `secret_key` line must be a single line ending in a closing `"`.

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

5. **Wire it into the agent.** Set the agent config value `searxng_url` by running
   (use whatever port you settled on):

       odysseus-code config set searxng_url http://localhost:8080

   Then tell the user that the `web_search` tool reads this value at startup, so
   they must restart odysseus-code for web search to become available.

As you finish each step, call complete_skill_step so your progress is tracked.
