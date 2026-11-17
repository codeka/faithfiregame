import { Component, input } from '@angular/core';
import { HudTone } from '../../hud.types';

@Component({
  selector: 'app-hud-divider',
  standalone: true,
  templateUrl: './hud-divider.component.html',
  styleUrl: './hud-divider.component.css',
  host: {
    '[attr.data-hud-tone]': 'tone()',
  },
})
export class HudDividerComponent {
  readonly tone = input<HudTone>('cyan');
}
