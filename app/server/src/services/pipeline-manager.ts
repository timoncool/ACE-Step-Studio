import { spawn, ChildProcess, execSync, exec } from 'child_process';
import { existsSync } from 'fs';
import { config } from '../config/index.js';

export type PipelineState =
  | 'stopped'
  | 'starting'
  | 'loading_model'
  | 'ready'
  | 'error'
  | 'restarting';

interface PipelineStatus {
  state: PipelineState;
  message: string;
  pid: number | null;
  restartCount: number;
  uptime: number | null;
  lastError: string | null;
}

class PipelineManager {
  private process: ChildProcess | null = null;
  private state: PipelineState = 'stopped';
  private message = '';
  private lastError: string | null = null;
  private restartCount = 0;
  private startedAt: number | null = null;
  private healthCheckTimer: ReturnType<typeof setInterval> | null = null;
  private isShuttingDown = false;
  private isSwitchingModel = false;
  private readyResolve: (() => void) | null = null;
  private readyReject: ((err: Error) => void) | null = null;
  private onRestartCallbacks: Array<() => void> = [];

  /** Register a callback that fires after pipeline restarts (model state reset). */
  onRestart(cb: () => void): void {
    this.onRestartCallbacks.push(cb);
  }

  getStatus(): PipelineStatus {
    return {
      state: this.state,
      message: this.message,
      pid: this.process?.pid ?? null,
      restartCount: this.restartCount,
      uptime: this.startedAt ? Date.now() - this.startedAt : null,
      lastError: this.lastError,
    };
  }

  /** Pause health checks during model switch to prevent zombie detection. */
  pauseHealthCheck() { this.isSwitchingModel = true; }
  resumeHealthCheck() { this.isSwitchingModel = false; }

  /** Spawn Python pipeline and wait for readiness. */
  async start(): Promise<void> {
    if (this.state === 'ready' || this.state === 'starting') return;

    const { pythonPath, aceStepDir, defaultModel, port, startupTimeout } = config.pipeline;

    if (!existsSync(pythonPath)) {
      throw new Error(`Python not found: ${pythonPath}`);
    }
    if (!existsSync(aceStepDir)) {
      throw new Error(`ACE-Step directory not found: ${aceStepDir}`);
    }

    this.state = 'starting';
    this.message = 'Spawning Python pipeline...';
    this.lastError = null;

    const initLlm = process.env.INIT_LLM !== 'false';
    const lmModel = process.env.LM_MODEL || 'acestep-5Hz-lm-0.6B';
    const lmBackend = process.env.LM_BACKEND || 'pt';

    const args = [
      '-u',
      '-m', 'acestep.acestep_v15_pipeline',
      '--config_path', defaultModel,
      '--port', String(port),
      '--init_service', 'true',
      '--init_llm', String(initLlm),
      ...(initLlm ? ['--lm_model_path', lmModel, '--backend', lmBackend] : []),
      '--enable-api',
      '--offload_to_cpu', 'true',
    ];

    console.log(`[Pipeline] Starting: ${pythonPath} ${args.join(' ')}`);
    console.log(`[Pipeline] CWD: ${aceStepDir}`);

    this.process = spawn(pythonPath, args, {
      cwd: aceStepDir,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],  // pipe stderr so we can detect Gradio ready signal
      env: {
        ...process.env,
        PYTHONUNBUFFERED: '1',
        PYTHONIOENCODING: 'utf-8',
        ACESTEP_SAVE_MEMORY: '1',  // skip storing intermediate GPU tensors between generations
        ACESTEP_OFFLOAD_TO_CPU: '1',  // LM unloads from GPU when DiT runs (persists across model switches)
        ACESTEP_LM_OFFLOAD_TO_CPU: '1',
      },
    });

    this.process.stdout!.on('data', (data: Buffer) => {
      const text = data.toString();
      process.stdout.write(`[Gradio] ${text}`);
      this.parseStdout(text);
    });

    // Pipe stderr: forward to process.stderr (tqdm still visible) and parse for readiness
    this.process.stderr!.on('data', (data: Buffer) => {
      const text = data.toString();
      process.stderr.write(text);
      this.parseStdout(text);   // Gradio 6 prints "Running on local URL:" to stderr
      this.parseStderr(text);
    });

    this.process.on('exit', (code, signal) => {
      console.log(`[Pipeline] Process exited: code=${code} signal=${signal}`);
      this.process = null;
      this.stopHealthCheck();

      if (this.readyReject) {
        this.readyReject(new Error(`Pipeline exited during startup (code=${code})`));
        this.readyResolve = null;
        this.readyReject = null;
      }

      if (!this.isShuttingDown) {
        this.state = 'error';
        this.lastError = `Process exited with code ${code}`;
        this.message = `Crashed (code ${code}). Restarting...`;
        this.scheduleRestart();
      }
    });

    this.process.on('error', (err) => {
      console.error(`[Pipeline] Spawn error:`, err);
      this.state = 'error';
      this.lastError = err.message;
      this.message = `Spawn failed: ${err.message}`;
    });

    // Wait for readiness with timeout — don't block Express startup
    return new Promise<void>((resolve) => {
      this.readyResolve = () => resolve();
      this.readyReject = () => resolve(); // resolve anyway so Express starts

      setTimeout(() => {
        if (this.state !== 'ready') {
          this.readyResolve = null;
          this.readyReject = null;
          console.warn(`[Pipeline] Startup timeout (${startupTimeout}ms) — pipeline still loading, Express will start anyway`);
          resolve();
        }
      }, startupTimeout);
    });
  }

  private parseStdout(text: string) {
    if (text.includes('GPU Memory:') || text.includes('GPU Configuration')) {
      this.message = 'GPU detected, configuring...';
    }
    if (text.includes('Loading checkpoint') || text.includes('Initializing DiT') || text.includes('Attempting to load')) {
      this.state = 'loading_model';
      this.message = 'Loading AI model...';
    }
    if (text.includes('Loading') && text.includes('shards')) {
      const match = text.match(/(\d+)%/);
      if (match) {
        this.message = `Loading model: ${match[1]}%`;
      }
    }
    if (text.includes('DiT model initialized successfully')) {
      this.message = 'DiT model loaded, loading language model...';
    }
    if (text.includes('Initializing 5Hz LM') || text.includes('loading 5Hz LM tokenizer')) {
      this.message = 'Loading language model...';
    }
    if (text.includes('Running on local URL') || text.includes('Uvicorn running on')) {
      this.onReady();
    }
  }

  private parseStderr(text: string) {
    if (text.includes('CUDA out of memory') || text.includes('OutOfMemoryError')) {
      this.lastError = 'CUDA out of memory';
      this.message = 'GPU memory exhausted';
    }
    if (text.includes('Address already in use')) {
      this.lastError = `Port ${config.pipeline.port} already in use`;
      this.message = this.lastError;
    }
  }

  private onReady() {
    if (this.state === 'ready') return;
    this.state = 'ready';
    this.message = 'Pipeline running';
    this.startedAt = Date.now();

    // Open browser only on first start, not on restarts.
    // Skip when NO_AUTO_BROWSER is set (Pinokio launcher handles tab opening itself).
    if (this.restartCount === 0 && process.env.NO_AUTO_BROWSER !== 'true') {
      const url = `http://localhost:${config.port}`;
      console.log(`[Pipeline] Opening browser: ${url}`);
      if (process.platform === 'win32') {
        exec(`start "" "${url}"`);
      } else if (process.platform === 'darwin') {
        exec(`open "${url}"`);
      } else {
        exec(`xdg-open "${url}"`);
      }
    }

    this.restartCount = 0;
    console.log('[Pipeline] Ready!');

    this.startHealthCheck();

    if (this.readyResolve) {
      this.readyResolve();
      this.readyResolve = null;
      this.readyReject = null;
    }
  }

  private startHealthCheck() {
    this.stopHealthCheck();
    let consecutiveFailures = 0;

    this.healthCheckTimer = setInterval(async () => {
      if (this.state !== 'ready' || this.isSwitchingModel) return;
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 5000);
        const res = await fetch(
          `http://localhost:${config.pipeline.port}/gradio_api/info`,
          { signal: controller.signal },
        );
        clearTimeout(timer);
        if (res.ok || res.status < 500) {
          consecutiveFailures = 0;
        } else {
          consecutiveFailures++;
          console.warn(`[Pipeline] Health check failed: HTTP ${res.status} (${consecutiveFailures}/3)`);
        }
      } catch {
        consecutiveFailures++;
        console.warn(`[Pipeline] Health check failed: no response (${consecutiveFailures}/3)`);
      }

      // Only act after 3 consecutive failures to avoid false positives during generation
      if (consecutiveFailures >= 3) {
        this.handleUnhealthy();
        consecutiveFailures = 0;
      }
    }, config.pipeline.healthCheckInterval);
  }

  private stopHealthCheck() {
    if (this.healthCheckTimer) {
      clearInterval(this.healthCheckTimer);
      this.healthCheckTimer = null;
    }
  }

  private handleUnhealthy() {
    if (this.state !== 'ready') return;
    this.state = 'error';
    this.lastError = 'Pipeline unresponsive (zombie)';
    this.message = 'Pipeline stopped responding. Killing and restarting...';
    console.error('[Pipeline] Zombie detected, killing process...');
    this.killProcess();
    this.scheduleRestart();
  }

  private scheduleRestart() {
    const { maxRestarts, backoffBase, backoffMax } = config.pipeline;

    if (this.restartCount >= maxRestarts) {
      this.state = 'error';
      this.message = `Max restarts (${maxRestarts}) exceeded. Manual intervention required.`;
      console.error(`[Pipeline] ${this.message}`);
      return;
    }

    const delay = Math.min(backoffBase * Math.pow(2, this.restartCount), backoffMax);
    this.restartCount++;
    this.state = 'restarting';
    this.message = `Restarting in ${Math.round(delay / 1000)}s (attempt ${this.restartCount}/${maxRestarts})`;
    console.log(`[Pipeline] ${this.message}`);

    setTimeout(() => {
      if (!this.isShuttingDown) {
        // Notify listeners to reset model state before restarting
        for (const cb of this.onRestartCallbacks) {
          try { cb(); } catch {}
        }
        this.start().catch(err => {
          console.error('[Pipeline] Restart failed:', err);
        });
      }
    }, delay);
  }

  private killProcess() {
    if (!this.process?.pid) return;
    try {
      if (process.platform === 'win32') {
        execSync(`taskkill /pid ${this.process.pid} /T /F`, { stdio: 'ignore' });
      } else {
        this.process.kill('SIGTERM');
        setTimeout(() => {
          try { this.process?.kill('SIGKILL'); } catch { /* already dead */ }
        }, 5000);
      }
    } catch {
      // Process may already be dead
    }
    this.process = null;
  }

  async shutdown(): Promise<void> {
    this.isShuttingDown = true;
    this.stopHealthCheck();
    console.log('[Pipeline] Shutting down...');
    this.killProcess();
    this.state = 'stopped';
    this.message = 'Stopped';
  }
}

export const pipelineManager = new PipelineManager();
