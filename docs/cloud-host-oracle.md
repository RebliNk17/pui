# Running a 24/7 Cloud Pui Host on Oracle Cloud (Always Free)

You can run Pui's headless cloud daemon (`daemon/`) on an **Oracle Cloud Always Free ARM VM** ($0/month) and connect to it from **Pui Mobile** over **Tailscale**—so your coding agents stay available 24/7 even when your Mac is asleep.

## What You Get (Always Free Tier)

- **Compute:** `VM.Standard.A1.Flex` (Ampere ARM64) — up to **4 OCPUs, 24 GB RAM**
- **Storage:** up to **200 GB** block/boot volume (100 GB recommended for multiple repos and build caches)
- **Networking:** Private Tailscale tailnet (`100.x.y.z:4817`) with zero public application ports exposed

---

## Step 1: Create the Oracle Cloud Always Free VM

1. Sign up for an [Oracle Cloud Free Tier](https://www.oracle.com/cloud/free/) account and choose a home region close to you (e.g. `il-jerusalem-1`, `eu-frankfurt-1`, or `eu-amsterdam-1`).
2. Generate a dedicated SSH keypair on your Mac:
   ```sh
   ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519_oci_pui -N "" -C "pui-cloud-oci"
   cat ~/.ssh/id_ed25519_oci_pui.pub
   ```
3. In the OCI Console, go to **Compute → Instances → Create instance**:
   - **Name:** `pui-cloud`
   - **Image:** **Canonical Ubuntu 24.04** (Free)
   - **Shape:** **Ampere → `VM.Standard.A1.Flex`** (*Always Free-eligible*) → expand **Shape details** and set **4 OCPUs** and **24 GB memory** (stays within the Always Free monthly allowance of 3,000 OCPU-hours / 18,000 GB-hours).
   - **Networking:** Select **Create new virtual cloud network** + **Create new public subnet** (`10.0.0.0/24`).
   - **Add SSH keys:** Select **Paste public key** and paste `~/.ssh/id_ed25519_oci_pui.pub`.
   - **Storage (Boot volume):** Enable custom size → set **100 GB** (VPU `10`, within the 200 GB Always Free storage limit).
   - Click **Create**.
4. **Assign an Ephemeral Public IPv4** (if the newly created subnet didn't assign one automatically):
   - Open the instance → **Networking** tab → click the **Primary VNIC** → **IP administration** tab → **Actions → Edit** on the primary private IP → select **Ephemeral public IP** → **Update**.
5. Verify SSH access:
   ```sh
   ssh -i ~/.ssh/id_ed25519_oci_pui ubuntu@<PUBLIC_IP> "uname -a && nproc && free -h"
   ```

---

## Step 2: Install Runtime Dependencies & Tailscale

SSH into the VM and install Node.js 22, Git, `ripgrep`, `fd`, build tools, and Tailscale:

```sh
sudo apt-get update
sudo apt-get install -y curl git ca-certificates gnupg ripgrep fd-find build-essential jq unzip
sudo ln -sf "$(command -v fdfind)" /usr/local/bin/fd

# Node.js 22 LTS
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs

# Tailscale
curl -fsSL https://tailscale.com/install.sh | sudo sh
sudo tailscale up --hostname=pui-cloud --ssh
```

Open the printed `https://login.tailscale.com/...` URL in your browser and click **Connect**. Confirm the assigned Tailscale IP:

```sh
tailscale ip -4
```

---

## Step 3: Install & Run `pui-daemon` via systemd

Clone this repository on the VM, install daemon dependencies, and enable the systemd service:

```sh
mkdir -p ~/workspaces
git clone https://github.com/RebliNk17/pui.git ~/pui
cd ~/pui/daemon
npm ci --omit=dev

sudo cp ~/pui/daemon/pui-daemon.service /etc/systemd/system/pui-daemon.service
sudo systemctl daemon-reload
sudo systemctl enable --now pui-daemon
sudo systemctl status pui-daemon
```

By default `pui-daemon` binds **only** to the VM's Tailscale IPv4 address (`100.x.y.z`) on port `4817`, auto-discovers every repo directory inside `~/workspaces/`, and persists configuration + bearer token in `~/.pui-daemon/config.json` (`chmod 600`).

---

## Step 4: Get the Pairing Link

`pui-daemon` generates a bearer token automatically on first launch (`~/.pui-daemon/config.json`) and prints its `pui://pair?...` link in the service logs. To print the link anytime on the VM:

```sh
cd ~/pui/daemon && npm run --silent pair
```

Or fetch it from your Mac over SSH:

```sh
ssh -i ~/.ssh/id_ed25519_oci_pui ubuntu@<PUBLIC_IP> "cd ~/pui/daemon && npm run --silent pair"
```

---

## Step 5: Pair with Pui Desktop & Sync Credentials

1. Open **Pui Desktop** → **Settings → Remote & Mobile** → **Cloud host (24/7 headless daemon)**.
2. Paste the `pui://pair?...` link and click **Connect**.
3. Use the Desktop-only controls on the card to finish setup over your encrypted Tailscale connection:
   - **Sync Provider Logins** — copies your Mac's `~/.pi/agent/auth.json` to `pui-cloud` (`chmod 600`) so all your AI providers and models work immediately.
   - **Setup Git & GitHub Key** — generates a dedicated, revocable `ed25519` SSH key *on the cloud VM* (your Mac's personal private key never leaves your Mac), sets your Git author name/email, and registers the VM's public key on GitHub via `gh ssh-key add` (or copies it to your clipboard).
   - **Clone Current Project** — clones the active Mac workspace's `origin` repository straight into `~/workspaces/` on `pui-cloud`.

*(Prefer manual setup? You can also run `~/pui/daemon/node_modules/.bin/pi /login` directly on the VM over SSH.)*

---

## Step 6: Pair with Pui Mobile

Open **Pui** on your Android phone → tap the top host chip → **Paste Link** → paste the same `pui://pair?...` link and tap **Connect**.

You can now switch between your Mac and **`pui-cloud`** from the top host pill, or pick **Where to run** directly inside **+ New session**.

## Updating `pui-daemon`

```sh
cd ~/pui && git pull && cd daemon && npm ci --omit=dev && sudo systemctl restart pui-daemon
```

## Optional: Docker / Railway Deployment

`daemon/Dockerfile` packages the same headless daemon for container platforms (set `PUI_DAEMON_BIND=all`, `PUI_DAEMON_TOKEN=<secret>`, and `PUI_DAEMON_TLS=1` behind your platform's HTTPS proxy).
