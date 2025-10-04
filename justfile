default: install start 
        
build:
	./gradlew :app:assembleDebug

install:
  ./gradlew :app:installDebug

start:
  adb shell monkey -p com.example.hellocompose -c android.intent.category.LAUNCHER 1
