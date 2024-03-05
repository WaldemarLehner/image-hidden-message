# image-hidden-message

Hide arbitrary binary data inside PNG-Images.

Look at these 2 pictures. For you as the viewer, they essentially look the same.
The right picture however has the HTTP/1.0 RFC, encoded as a hidden message, inside it.

| Image with no data                          | Image containing HTTP/1.0 RFC                |
| ------------------------------------------- | -------------------------------------------- |
| ![](./README-source/exampleImageNoData.png) | ![](./README-source/exampleImageRfcData.png) |

## Usage

Encode data by piping into the encode command:

```sh
cat someData.tgz | image-hidden-message encode ./sourceImage.png > ./imageWithMessage.png
# or
image-hidden-message encode ./sourceImage.png --message="mySecretMessage" > ./imageWithMessage.png
```

Get data from an image by piping the image into the decode command:

```sh
cat imageWithMessage.png | image-hidden-message > hiddenPayload
```

You can try to decode the image from above!

```sh
curl https://raw.githubusercontent.com/WaldemarLehner/image-hidden-message/main/README-source/exampleImageRfcData.png | image-hidden-message > message.txt
```

## Build

```sh
# Assumes cargo / rust(up) is installed
cargo build --release
```
