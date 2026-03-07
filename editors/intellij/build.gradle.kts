plugins {
    id("java")
    id("org.jetbrains.intellij.platform")
    id("org.jetbrains.grammarkit") version "2022.3.2.2"
}

group = "eu.spawnlink"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdea("2024.3")
    }
}

intellijPlatform {
    pluginConfiguration {
        name = "Relux"
        version = project.version.toString()
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

sourceSets {
    main {
        java {
            srcDirs("src/main/java", "src/main/gen")
        }
    }
}

tasks {
    generateLexer {
        sourceFile.set(file("src/main/java/eu/spawnlink/relux/ReluxLexer.flex"))
        targetOutputDir.set(file("src/main/gen/eu/spawnlink/relux"))
        purgeOldFiles.set(true)
    }

    compileJava {
        dependsOn(generateLexer)
    }
}
