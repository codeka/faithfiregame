import { Component } from '@angular/core';

@Component({
  selector: 'app-hud',
  standalone: true,
  template: `
    <div class="hud-banner">
      <span>Faith &amp; Fire — In-Game HUD</span>
    </div>
  `,
  styles: [
    `
      .hud-banner {
        position: fixed;
        top: 12px;
        left: 12px;
        pointer-events: auto;
        padding: 6px 12px;
        border-radius: 6px;
        background: rgba(16, 20, 28, 0.85);
        border: 1px solid rgba(255, 255, 255, 0.15);
      }
    `,
  ],
})
export class HudComponent {}
