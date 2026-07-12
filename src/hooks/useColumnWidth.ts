import { useEffect, useRef, useState } from 'react'

// 드래그로 조절하는 열 너비. localStorage에 화면별로 저장한다.
export function useColumnWidth(key: string, def = 280, min = 180, max = 680) {
  const clamp = (n: number) => Math.min(max, Math.max(min, n || def))
  const [width, setWidth] = useState(() => {
    const s = localStorage.getItem(key)
    return clamp(s ? Number(s) : def)
  })
  const drag = useRef<{ x: number; w: number } | null>(null)
  const widthRef = useRef(width)
  useEffect(() => { widthRef.current = width }, [width])

  const onMouseDown = (e: React.MouseEvent) => {
    drag.current = { x: e.clientX, w: widthRef.current }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    e.preventDefault()
  }

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (!drag.current) return
      setWidth(clamp(drag.current.w + (e.clientX - drag.current.x)))
    }
    const up = () => {
      if (!drag.current) return
      drag.current = null
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
      localStorage.setItem(key, String(widthRef.current))
    }
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', up)
    return () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])

  return { width, onMouseDown }
}
