import { Component, computed, input } from '@angular/core';
import { HudBadgeComponent } from '../../primitives/hud-badge/hud-badge.component';
import { HudDividerComponent } from '../../primitives/hud-divider/hud-divider.component';
import { HudFrameComponent } from '../../primitives/hud-frame/hud-frame.component';

function formatCoord(value: number): string {
  const rounded = Math.round(value) || 0;
  if (rounded >= 0 && rounded < 10) {
    return String(rounded).padStart(2, '0');
  }
  return String(rounded);
}

@Component({
  selector: 'app-hud-area-telemetry',
  standalone: true,
  imports: [HudFrameComponent, HudBadgeComponent, HudDividerComponent],
  templateUrl: './area-telemetry.component.html',
  styleUrl: './area-telemetry.component.css',
  host: {
    '[attr.data-area-id]': 'areaId() || null',
  },
})
export class AreaTelemetryComponent {
  readonly areaName = input<string>('');
  readonly areaId = input<string>('');
  readonly playerPos = input<[number, number, number] | null>(null);

  readonly displayAreaName = computed(() => this.areaName() || 'Cryo Prison');

  readonly coordsLabel = computed<string | null>(() => {
    const pos = this.playerPos();
    if (!pos) {
      return null;
    }
    return `${formatCoord(pos[0])} · ${formatCoord(pos[2])}`;
  });
}
