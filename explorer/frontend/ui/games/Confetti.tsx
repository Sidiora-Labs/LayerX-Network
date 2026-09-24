import { Box } from '@chakra-ui/react';
import React, { useState, useEffect } from 'react';

const Confetti = () => {
  const [ shouldGenerate, setShouldGenerate ] = useState(true);

  useEffect(() => {
    const timer = setTimeout(() => {
      setShouldGenerate(false);
    }, 5000);

    return () => clearTimeout(timer);
  }, []);

  const getRandomShape = () => {
    const shapes = [
      // Circle
      { borderRadius: '50%' },
      // Triangle
      { clipPath: 'polygon(50% 0%, 0% 100%, 100% 100%)' },
      // Star
      { clipPath: 'polygon(50% 0%, 61% 35%, 98% 35%, 68% 57%, 79% 91%, 50% 70%, 21% 91%, 32% 57%, 2% 35%, 39% 35%)' },
      // Diamond
      { clipPath: 'polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%)' },
      // Square (default)
      { borderRadius: '0' },
    ];
    return shapes[Math.floor(Number(Math.random()) * shapes.length)];
  };

  const createSplash = (centerX: number, centerY: number, delay: number) => {
    const confettiCount = 100;
    return Array.from({ length: confettiCount }).map((_, i) => {
      const angle = Number(Math.random()) * 360;
      const distance = Number(Math.random()) * 200 + 100;
      const shape = getRandomShape();
      return {
        id: `${ centerX }-${ centerY }-${ i }`,
        left: `${ centerX }%`,
        top: `${ centerY }%`,
        angle,
        distance,
        animationDuration: `${ Number(Math.random()) * 1 + 1 }s`,
        animationDelay: `${ delay }s`,
        size: `${ Number(Math.random()) * 10 + 5 }px`,
        color: `hsl(${ Number(Math.random()) * 360 }, 100%, 50%)`,
        transform: `translate(
          calc(-50% + ${ Math.cos(angle * Math.PI / 180) * distance }px),
          calc(-50% + ${ Math.sin(angle * Math.PI / 180) * distance }px)
        )`,
        shape,
      };
    });
  };

  const splashes = shouldGenerate ? [
    createSplash(50, 50, 0), // center
    createSplash(20, 30, Number(Math.random()) * 3), // top left
    createSplash(80, 30, Number(Math.random()) * 3), // top right
    createSplash(20, 70, Number(Math.random()) * 3), // bottom left
    createSplash(80, 70, Number(Math.random()) * 3), // bottom right
  ].flat() : [];

  return (
    <>
      <style>
        { `
          @keyframes splash {
            0% {
              transform: translate(-50%, -50%) scale(0);
              opacity: 0;
            }
            1% {
              opacity: 1;
            }
            80% {
              opacity: 0.9;
            }
            100% {
              transform: var(--final-transform) scale(1);
              opacity: 0;
            }
          }
        ` }
      </style>
      <Box
        position="fixed"
        top="0"
        left="0"
        width="100%"
        height="100%"
        pointerEvents="none"
        zIndex="9999"
      >
        { splashes.map((piece) => (
          <Box
            key={ piece.id }
            position="absolute"
            left={ piece.left }
            top={ piece.top }
            width={ piece.size }
            height={ piece.size }
            backgroundColor={ piece.color }
            opacity={ 0 }
            animation={ `splash ${ piece.animationDuration } ease-out ${ piece.animationDelay } forwards` }
            style={{
              transformOrigin: 'center',
              '--final-transform': piece.transform,
              ...piece.shape,
            } as unknown as React.CSSProperties}
          />
        )) }
      </Box>
    </>
  );
};

export default Confetti;
