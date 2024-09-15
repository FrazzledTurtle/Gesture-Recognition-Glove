// Libraries
#include <Arduino.h>
#include <Adafruit_MPU6050.h>
#include <Adafruit_Sensor.h>
#include <WiFi.h>
#include <WiFiClient.h>
#include <WebServer.h>
#include <index.h>

// Expose port 80
WebServer server(80);

// Network Credentials 
const char *ssid = "Wi-Fi name";
const char *password = "Wi-Fi pass";

// Global Variables
#define LED 2
#define Flex_Sensor1 34
#define Flex_Sensor2 35
#define Flex_Sensor3 33
#define Flex_Sensor4 32
#define Flex_Sensor5 39

// Local Variables
const float VCC = 3.3;
const float R_Divider = 10000.0;
float R_Straight1, R_Straight2, R_Straight3, R_Straight4, R_Straight5 = 12300.0;
float R_Bend1, R_Bend2, R_Bend3, R_Bend4, R_Bend5 = 29000.0;
float flexV1, flexV2, flexV3, flexV4, flexV5, flexR1, flexR2, flexR3, flexR4, flexR5 = 0.0;
String storeReadings = "";
String OldstoreReadings = "0";

// Accelerometer and Gyroscope Variables
unsigned long lastTime, lastTimeAcc = 0;
unsigned long gyroDelay = 10;
unsigned long temperatureDelay = 1000;
unsigned long accelerometerDelay = 200;
float gyroX, gyroY, gyroZ;
float accX, accY, accZ;

// Creating an object for the MPU6050
Adafruit_MPU6050 mpu;
sensors_event_t a, g, temp;

// Gyroscope sensor deviation
float gyroXerror = 0.09;
float gyroYerror = 0.04;
float gyroZerror = 0.02;

// Initialization of module
void initMPU()
{
  if (!mpu.begin())
  {
    Serial.println("Failed to find the MPU6050 chip.");
    delay(10);
  }
  Serial.println("MPU6050 was found!");
  Serial.println("\n");
}

// Connection to local network.
void initWiFi()
{
  WiFi.mode(WIFI_STA);
  WiFi.begin(ssid, password);
  Serial.println("");
  Serial.print("Connecting to WiFi");
  while (WiFi.status() != WL_CONNECTED)
  {
    Serial.print(".");
    delay(1000);
  }
  Serial.println("\n");
  Serial.println(WiFi.localIP());
}

// This routing is executed when you open its IP in browser
void handleRoot()
{
  String s = index_html; // Read HTML contents
  server.send(200, "text/html", s); // Sends the Web Page
}

// Calibrating the flex sensors
void Calibration()
{
  Serial.println("Starting Calibration.\n");
    while(millis()<15000)
  {
    float Flex_Value1 = analogRead(Flex_Sensor1);
    float Flex_Value2 = analogRead(Flex_Sensor2);
    float Flex_Value3 = analogRead(Flex_Sensor3);
    float Flex_Value4 = analogRead(Flex_Sensor4);
    float Flex_Value5 = analogRead(Flex_Sensor5);
    if (Flex_Value1<R_Straight1)
    {
      R_Straight1=Flex_Value1;
    }
    if(Flex_Value1>R_Bend1)
    {
      R_Bend1=Flex_Value1;
    }
    if (Flex_Value2<R_Straight2)
    {
      R_Straight2=Flex_Value2;
    }
    if(Flex_Value2>R_Bend2)
    {
      R_Bend2=Flex_Value2;
    }
     if (Flex_Value3<R_Straight3)
    {
      R_Straight3=Flex_Value3;
    }
    if(Flex_Value3>R_Bend3)
    {
      R_Bend3=Flex_Value3;
    }
     if (Flex_Value4<R_Straight4)
    {
      R_Straight4=Flex_Value4;
    }
    if(Flex_Value4>R_Bend4)
    {
      R_Bend4=Flex_Value4;
    }
    if (Flex_Value5<R_Straight5)
    {
      R_Straight5=Flex_Value5;
    }
    if(Flex_Value5>R_Bend5)
    {
      R_Bend5=Flex_Value5;
    }
  }
  Serial.println("Calibration was successful.\n");
  delay(400);
}

// Gathering data of Gyroscope
void getGyroReadings()
{
  mpu.getEvent(&a, &g, &temp);
  float gyroX_temp = g.gyro.x;
  if(abs(gyroX_temp) > gyroXerror)
  {
    gyroX += gyroX_temp/50.00;
  }

  float gyroY_temp = g.gyro.y;
  if(abs(gyroY_temp) > gyroYerror)
  {
    gyroY += gyroY_temp/70.00;
  }

  float gyroZ_temp = g.gyro.z;
  if(abs(gyroZ_temp) > gyroZerror)
  {
    gyroZ += gyroZ_temp/90.00;
  }
}
// Gathering data of Accelerometer
void getAccReadings()
{
  mpu.getEvent(&a, &g, &temp);
  
  //Get the current acceleration values
  accX = a.acceleration.x;
  accY = a.acceleration.y;
  accZ = a.acceleration.z;
}

// Gathering data of Flex Resistances
void getFlexReadings()
{ 
  float Flex_Value1 = analogRead(Flex_Sensor1);
  float Flex_Value2 = analogRead(Flex_Sensor2);
  float Flex_Value3 = analogRead(Flex_Sensor3);
  float Flex_Value4 = analogRead(Flex_Sensor4);
  float Flex_Value5 = analogRead(Flex_Sensor5);
  flexV1 = Flex_Value1 * VCC / 4095.0;
  flexR1 = R_Divider * (VCC / flexV1 - 1.0);
  flexV2 = Flex_Value2 * VCC / 4095.0;
  flexR2 = R_Divider * (VCC / flexV2 - 1.0);
  flexV3 = Flex_Value3 * VCC / 4095.0;
  flexR3 = R_Divider * (VCC / flexV3 - 1.0);
  flexV4 = Flex_Value4 * VCC / 4095.0;
  flexR4 = R_Divider * (VCC / flexV4 - 1.0);
  flexV5 = Flex_Value5 * VCC / 4095.0;
  flexR5 = R_Divider * (VCC / flexV5 - 1.0);
}

// Fixing multiple printing of the same gestures
boolean printGestureReadings()
{
  if (!OldstoreReadings.equals(storeReadings))
  {
    OldstoreReadings = storeReadings;
    return true;
  }
  return false;
}
// Poor man's datasets
void getGestureReadings()
{
  if (flexR1>=3000 && flexR1<=8500 && flexR2>=3000 && flexR2<=8500 && flexR3>=3000 && flexR3<=8500 && flexR4>=3000 && flexR4<=8500 && flexR5>=3000 && flexR5<=8500)
  {
    storeReadings = "IDLE";
  }
  if(flexR1>=7000 && flexR1<=18000 && flexR2>=3000 && flexR2<=7000 && flexR3>=7000 && flexR3<=18000 && flexR4>=7000 && flexR4<=18000 && flexR5>=7000 && flexR5<=18000)
  {
    storeReadings = "1";
  }
  if(flexR1>=7000 && flexR1<=18000 && flexR2>=3000 && flexR2<=7000 && flexR3>=3000 && flexR3<=7000 && flexR4>=7000 && flexR4<=18000 && flexR5>=7000 && flexR5<=18000)
  {
    storeReadings = "2";
  }
  if(flexR1>=3000 && flexR1<=7000 && flexR2>=3000 && flexR2<=7000 && flexR3>=3000 && flexR3<=7000 && flexR4>=7000 && flexR4<=18000 && flexR5>=7000 && flexR5<=18000)
  {
    storeReadings = "3";
  }
   if(flexR1>=3000 && flexR1<=7000 && flexR2>=3000 && flexR2<=7000 && flexR3>=7000 && flexR3<=18000 && flexR4>=7000 && flexR4<=18000 && flexR5>=3000 && flexR5<=8500)
  {
    storeReadings = "I LOVE YOU!";
  }
  if(flexR1>=3000 && flexR1<=7000 && flexR2>=7000 && flexR2<=18000 && flexR3>=7000 && flexR3<=18000 && flexR4>=7000 && flexR4<=18000 && flexR5>=3000 && flexR5<=8500)
  {
    storeReadings = "Name";
  }
  if(accX<=(-3.50) && accX>=(-10.50))
  {
    storeReadings = "My!";
  }
   if(accX>=(5.50) && accX<=(10.50))
  {
    storeReadings = "HELLO!";
  }
  if (printGestureReadings())
  {
       Serial.print(storeReadings + "\n");
  }
  // Sends the value only to client ajax request
  server.send(200, "text/plane", storeReadings); 
}
// After boot
void setup() 
{
  Serial.begin(115200);
  pinMode(LED, OUTPUT);
  initMPU();
  Calibration();
  initWiFi();

  server.on("/", handleRoot); //This is diplay page
  // To get update for gesture reading only
  server.on("/read_gesture", getGestureReadings); 
  server.begin();
  Serial.println("HTTP server started!\n");
  digitalWrite(LED, HIGH);
}

// Main Loop
void loop() 
{
  getGyroReadings();
  getAccReadings();
  getFlexReadings();
  getGestureReadings();
  server.handleClient();
  delay(500);
}