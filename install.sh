#!/bin/bash

InstallDir=$1

read -p "Install to $InstallDir? [y/n] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]
then
    echo Installing to: $InstallDir
    mkdir -p $InstallDir
    mkdir -p $InstallDir/backup
    touch $InstallDir/config.toml
    cp ./target/release/stencil $InstallDir/stencil
fi