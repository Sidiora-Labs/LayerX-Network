-keep class com.sidiora.layerx.sdk.** { *; }
-keep class com.sidiora.layerx.android.** { *; }
-keepattributes Signature,InnerClasses,EnclosingMethod,RuntimeVisibleAnnotations
-dontwarn com.fasterxml.jackson.databind.ext.**
-dontwarn org.bouncycastle.jsse.**
-dontwarn javax.naming.**
# HttpProductionTransport is the JVM-only transport; Android routes through
# AndroidHttpTransport, so java.net.http never loads on device.
-dontwarn java.net.http.**
# javac 21 references MatchException in exhaustive-switch fallbacks and the JVM
# SDK's stream subscription spins a virtual thread; neither type exists on
# API 34 and neither code path runs on device.
-dontwarn java.lang.MatchException
-dontwarn java.lang.Thread$Builder$OfVirtual
