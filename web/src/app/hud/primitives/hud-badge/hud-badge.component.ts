import { Component, input } from '@angular/core';
import { HudTone } from '../../hud.types';

@Component({
  selector: 'app-hud-badge',
  standalone: true,
  templateUrl: './hud-badge.component.html',
  styleUrl: './hud-badge.component.css',
  host: {
    '[attr.data-hud-tone]': 'tone()',
  },
})
export class HudBadgeComponent {
  readonly tone = input<HudTone>('cyan');
}
