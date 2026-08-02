import { DaraText } from '../components/DaraText.tsx'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../components/dara-text-types.ts'

export function CardSource({ value }: { value: string }) {
  return (
    <DaraText
      as="p"
      className="source"
      tone={DaraTextTone.Muted}
      variant={DaraTextVariant.Supporting}
    >
      Source: {value}
    </DaraText>
  )
}
