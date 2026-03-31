#include <Adafruit_NeoPixel.h>
/*
#include "AwesomeDots.h"

#define NUM_LEDS 256
#define LED_PIN  5
Adafruit_NeoPixel leds(NUM_LEDS, LED_PIN, NEO_GRB + NEO_KHZ800);
AwesomeDots fx(leds, NUM_LEDS);

void setup() {
  leds.begin();
  fx.begin(64);
}

void loop() {
  fx.tick();
}
*/

Adafruit_NeoPixel leds(256, 1, NEO_GRB + NEO_KHZ800);
void setup() {
  leds.begin();
  leds.setBrightness(16);
}

int h = 0, s = 255, r = 0, speed = 0; 

bool up = false; 

void loop() {
  if (true) newf();
  else {

    leds.rainbow(h, r, s); 
    leds.show(); 
    h += 64 * speed;
    if (!up) {
        if (s == 0){ up = true; r++; speed += 4; if (r > 8){r=0; speed = 0;}}
        else s--;
    } else {
        if (s == 255){ up = false;}
        else s++;
    }
}}

bool invert = false;
int wrap(int x){
   if (!invert){
    if (x < 0)
      return 256 + x;
    return x;
  }else
  {
    if (x > 255)
      return  256 + x;
    return x;
  }
}
int t  = 0;
int isolated = 1;
long unsigned int delay = 0;
void newf() {
    int i = t % 256;
    for (int j = 0; j < 8; j++){
      i = (i + (j * 32)) % 256;
      leds.setPixelColor(i, leds.Color(255, 55, 255));
      if (!invert){
      leds.setPixelColor(wrap(i - 1), leds.Color(100, 20, 100));
      leds.setPixelColor(wrap(i - 2), leds.Color(50, 10, 50));
      leds.setPixelColor(wrap(i - 3), leds.Color(25, 5, 25));
      leds.setPixelColor(wrap(i - 4), leds.Color(0, 0, 0));
      }else
      {
      leds.setPixelColor(wrap(i + 1), leds.Color(100, 20, 100));
      leds.setPixelColor(wrap(i + 2), leds.Color(50, 10, 50));
      leds.setPixelColor(wrap(i + 3), leds.Color(25, 5, 25));
      leds.setPixelColor(wrap(i + 4), leds.Color(0, 0, 0));        
      }
    }
    if (i == 255){delay++; isolated++;} 

    if ((isolated == 9) ){
      isolated = 0;
      invert = !invert;
    }

    if (!invert) t++; else t--;
    delay(delay);
    leds.show(); 
}