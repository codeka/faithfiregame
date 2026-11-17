import { Component, input } from '@angular/core';
import { HudTone } from '../../hud.types';

@Component({
  selector: 'button[appHudButton], a[appHudButton]',
  standalone: true,
  templateUrl: './hud-button.component.html',
  styleUrl: './hud-button.component.css',
  host: {
    '[attr.data-hud-tone]': 'tone()',
    '[attr.data-hud-size]': 'size()',
    '[class.is-active]': 'active()',
  },
})
export class HudButtonComponent {
  readonly tone = input<HudTone>('cyan');
  readonly size = input<'sm' | 'md'>('md');
  readonly active = input<boolean>(false);
}
