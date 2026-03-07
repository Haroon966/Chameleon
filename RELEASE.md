# Creating a release

1. **Commit and push all changes to `main`**
   ```bash
   git add -A
   git status   # review
   git commit -m "Release v0.1.1: Windows support, universal install"
   git push origin main
   ```

2. **Create and push a version tag**  
   This triggers the GitHub Actions release workflow (builds Linux, macOS, Windows and publishes the release).
   ```bash
   git tag v0.1.1
   git push origin v0.1.1
   ```

3. **Wait for the workflow**  
   On GitHub: **Actions** → **Release** → the run for your tag. It will build all targets and create the release with assets.

4. **Done**  
   Users can install with:
   - **Linux/macOS:** `curl -sSL https://raw.githubusercontent.com/Haroon966/Chameleon/main/install.sh | sh`
   - **Windows:** `irm https://raw.githubusercontent.com/Haroon966/Chameleon/main/install.ps1 | iex`

Ensure `Cargo.toml` `version` matches the tag (e.g. `0.1.1` for tag `v0.1.1`).
